import { ApiError, type Transport } from './errors';
import { fixtures, defaultPolicy, demoSchema, now, uuid } from './fixtures';
import { analyzeDemo } from './mock-guard';
import { createMockDiscovery } from './mock-discovery';
import { endpointChanged } from '../lib/connection';
import { isCidr } from '../lib/utils';
import type {
  Approval,
  Cluster,
  HistoryEntry,
  Policy,
  QueryResult,
  User,
} from './types';
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const fail = (
  code: string,
  message: string,
  status = 400,
  details: unknown = null,
): never => {
  throw new ApiError(code, message, status, details);
};
const required = <T>(value: T | undefined): T =>
  value ?? fail('not_found', 'This item no longer exists.', 404);
/** A session-isolated, in-memory implementation of the v1 contract. */
export function createMockTransport({
  delay = 180,
}: { delay?: number } = {}): Transport {
  const state = fixtures();
  const passwords = new Map(
    state.users.map((user) => [user.id, 'demo-password']),
  );
  const policies = new Map(
    state.clusters.map((cluster) => [
      cluster.id,
      defaultPolicy(cluster.environment),
    ]),
  );
  let session: User | undefined;
  let network = {
    allowed_cidrs: ['10.0.0.0/8', '127.0.0.1/32', '::1/128'],
    trust_proxy_headers: false,
  };
  const cancelled = new Set<string>();
  const running = new Map<string, string>();
  const clusterAccess = (cluster: Cluster) =>
    session?.org_role === 'admin'
      ? 'admin'
      : (state.grants
          .filter(
            (grant) =>
              grant.user.id === session?.id &&
              (grant.scope_id === cluster.id ||
                grant.scope_id === cluster.project_id) &&
              (!grant.expires_at ||
                new Date(grant.expires_at).getTime() > Date.now()),
          )
          .map((grant) => grant.level)
          .sort(
            (a, b) =>
              ['read', 'write', 'admin'].indexOf(b) -
              ['read', 'write', 'admin'].indexOf(a),
          )[0] ?? null);
  const admin = () => {
    if (session?.org_role !== 'admin')
      fail('forbidden', 'Administrator access is required.', 403);
  };
  const clusterAdmin = (cluster: Cluster) => {
    if (clusterAccess(cluster) !== 'admin')
      fail('forbidden', 'Cluster administrator access is required.', 403);
  };
  const activeAdminGrants = () =>
    state.grants.filter(
      (grant) =>
        grant.user.id === session?.id &&
        grant.level === 'admin' &&
        (!grant.expires_at || Date.parse(grant.expires_at) > Date.now()),
    );
  const canAdminScope = (scope: string, scopeId: string) =>
    session?.org_role === 'admin' ||
    activeAdminGrants().some(
      (own) =>
        (own.scope === scope && own.scope_id === scopeId) ||
        (scope === 'cluster' &&
          own.scope === 'project' &&
          state.clusters.some(
            (cluster) =>
              cluster.id === scopeId && cluster.project_id === own.scope_id,
          )),
    );
  const requireScopeAdmin = () => {
    if (session?.org_role !== 'admin' && !activeAdminGrants().length)
      fail('forbidden', 'Administrator access on a scope is required.', 403);
  };
  const validateText = (value: unknown): void => {
    if (
      typeof value === 'string' &&
      [...value].some((c) => c < ' ' && !['\t', '\n', '\r'].includes(c))
    )
      fail('validation', 'Text contains prohibited control characters');
    if (Array.isArray(value)) value.forEach(validateText);
    else if (value && typeof value === 'object')
      Object.entries(value).forEach(([key, value]) => {
        validateText(key);
        validateText(value);
      });
  };
  const audit = (
    action: string,
    target_type: string,
    target_id: string,
    details: unknown,
  ) =>
    state.audit.unshift({
      id: crypto.randomUUID(),
      actor: session ? { id: session.id, email: session.email } : null,
      action,
      target_type,
      target_id,
      ip: '10.12.0.24',
      details,
      created_at: now(),
    });
  function history(
    user: User,
    cluster: Cluster,
    sql: string,
    status: HistoryEntry['status'],
    analysis: ReturnType<typeof analyzeDemo>,
    result?: QueryResult,
    error: string | null = null,
  ) {
    state.history.unshift({
      id: crypto.randomUUID(),
      cluster_id: cluster.id,
      cluster_name: cluster.name,
      user_id: user.id,
      user_email: user.email,
      sql,
      verdict: analysis.verdict,
      status,
      row_count: result?.row_count ?? null,
      elapsed_ms: result?.elapsed_ms ?? null,
      error,
      created_at: now(),
    });
    audit('query.execute', 'cluster', cluster.id, { sql, status });
  }
  function result(
    cluster: Cluster,
    sql: string,
    query_id: string,
  ): QueryResult {
    const policy = required(policies.get(cluster.id));
    const analysis = analyzeDemo(sql, policy);
    const write = analysis.verdict === 'requires_approval';
    const limit = Number(
      analysis.rewritten_sql?.match(/\blimit\s+(\d+)/i)?.[1] ?? 100,
    );
    const names = [
      'Maya Patel',
      'Oliver Davis',
      'Sofia Torres',
      'Noah Williams',
      'Ava Thompson',
      'Liam Chen',
    ];
    const rows = Array.from(
      { length: Math.min(limit, policy.max_rows, 10000) },
      (_, i) => [
        uuid(2000 + i),
        names[i % 6],
        '••••••',
        i % 7 === 0
          ? null
          : { plan: i % 2 ? 'pro' : 'team', region: 'us-east-1' },
        new Date(Date.now() - i * 3600000).toISOString(),
      ],
    );
    return {
      query_id,
      columns: write
        ? []
        : [
            { name: 'id', type_name: 'uuid', masked: false },
            { name: 'name', type_name: 'text', masked: false },
            { name: 'email', type_name: 'text', masked: true },
            { name: 'metadata', type_name: 'jsonb', masked: false },
            { name: 'created_at', type_name: 'timestamptz', masked: false },
          ],
      rows: write ? [] : rows,
      row_count: write ? 0 : rows.length,
      truncated: !write && limit > policy.max_rows,
      affected_rows: write ? 1 : null,
      elapsed_ms: 37,
      routed_to:
        !write && cluster.replica_host && policy.route_reads_to_replica
          ? 'replica'
          : 'primary',
      executed_sql: analysis.rewritten_sql ?? sql,
      analysis,
    };
  }
  // One colleague's pending request and one approved request make the review flow demoable.
  const sampleCluster = required(state.clusters[0]);
  for (let i = 0; i < 4; i++) {
    const requester = required(state.users[i === 0 ? 2 : 0]);
    const sql = "UPDATE users SET name = 'Maya Patel' WHERE id = 42";
    state.approvals.push({
      id: uuid(800 + i),
      cluster_id: sampleCluster.id,
      cluster_name: sampleCluster.name,
      requester: {
        id: requester.id,
        email: requester.email,
        name: requester.name,
      },
      sql,
      reason:
        i === 0
          ? 'Correct a customer name after a verified support request.'
          : 'Repair an incorrect display name.',
      analysis: analyzeDemo(sql, required(policies.get(sampleCluster.id))),
      status:
        (['pending', 'approved', 'executing', 'failed'] as const)[i] ??
        'pending',
      error: i === 3 ? 'Target database unavailable' : null,
      sql_truncated: false,
      reviewer:
        i === 0
          ? null
          : {
              id: required(state.users[1]).id,
              email: required(state.users[1]).email,
              name: required(state.users[1]).name,
            },
      review_note: i === 0 ? null : 'Verified the target record.',
      result: null,
      created_at: now(),
      reviewed_at: i === 0 ? null : now(),
      executed_at: i === 3 ? now() : null,
      expires_at: new Date(Date.now() + 86400000).toISOString(),
    });
  }
  function page<T extends { id: string; created_at: string }>(
    items: T[],
    query: URLSearchParams,
    maxLimit = 200,
  ) {
    const limit = Number(query.get('limit') ?? 50);
    if (!Number.isInteger(limit) || limit < 1 || limit > maxLimit)
      fail('validation', `Limit must be 1..=${maxLimit}`);
    const sorted = [...items].sort(
      (a, b) =>
        b.created_at.localeCompare(a.created_at) || b.id.localeCompare(a.id),
    );
    const cursor = query.get('cursor');
    let remaining = sorted;
    if (cursor) {
      let boundary: { created_at: string; id: string };
      try {
        boundary = JSON.parse(
          atob(cursor.replaceAll('-', '+').replaceAll('_', '/')),
        ) as typeof boundary;
      } catch {
        return fail('validation', 'Invalid cursor');
      }
      if (!boundary.created_at || !boundary.id)
        fail('validation', 'Invalid cursor');
      remaining = sorted.filter(
        (item) =>
          item.created_at < boundary.created_at ||
          (item.created_at === boundary.created_at && item.id < boundary.id),
      );
    }
    const batch = remaining.slice(0, limit),
      last = batch.at(-1);
    return {
      items: batch,
      next_cursor:
        remaining.length > limit && last
          ? btoa(JSON.stringify({ created_at: last.created_at, id: last.id }))
              .replaceAll('+', '-')
              .replaceAll('/', '_')
              .replace(/=+$/, '')
          : null,
    };
  }
  const discovery = createMockDiscovery({
    clusters: () => state.clusters,
    projects: () => state.projects,
    delay,
    addCluster: (body) => {
      const { password, ...fields } = body;
      void password;
      const cluster = {
        ...fields,
        id: crypto.randomUUID(),
        health: {
          ...sampleCluster.health,
          status: 'unknown',
          latency_ms: null,
          checked_at: null,
        },
        my_access: 'admin',
        created_at: now(),
        updated_at: now(),
      } as Cluster;
      state.clusters.push(cluster);
      policies.set(cluster.id, defaultPolicy(cluster.environment));
      audit('discovery.import', 'cluster', cluster.id, { name: cluster.name });
      return cluster;
    },
  });
  async function route(path: string, init: RequestInit = {}): Promise<unknown> {
    const url = new URL(path, 'http://mock');
    const segments = url.pathname.split('/').filter(Boolean);
    const [resource, id, action] = segments;
    const method = init.method ?? 'GET';
    const body: Record<string, unknown> = init.body
      ? (JSON.parse(String(init.body)) as Record<string, unknown>)
      : {};
    validateText(body);
    if (resource === 'auth' && id === 'login') {
      const user = state.users.find(
        (user) => user.email === body.email && !user.disabled,
      );
      if (!user || passwords.get(user.id) !== body.password)
        fail('unauthenticated', 'Email or password is incorrect.', 401);
      session = user;
      return { user };
    }
    if (!session || session.disabled)
      fail('unauthenticated', 'Sign in to continue.', 401);
    const actor = required(session);
    if (resource === 'discovery') {
      admin();
      return discovery.route(url, method, body);
    }
    if (resource === 'auth') {
      if (id === 'me') return { user: session };
      if (id === 'logout') {
        session = undefined;
        return undefined;
      }
      if (id === 'change-password') {
        if (passwords.get(actor.id) !== body.current_password)
          fail('validation', 'Current password is incorrect.');
        if (String(body.new_password ?? '').length < 12)
          fail('validation', 'Use at least 12 characters.');
        passwords.set(actor.id, String(body.new_password));
        return undefined;
      }
    }
    if (resource === 'overview') {
      const accessible = state.clusters.filter((cluster) =>
        clusterAccess(cluster),
      );
      const entries = state.history.filter(
        (row) =>
          accessible.some((cluster) => cluster.id === row.cluster_id) &&
          (session?.org_role === 'admin' || row.user_id === session?.id),
      );
      return {
        discovery: actor.org_role === 'admin' ? discovery.summary() : null,
        clusters_total: accessible.length,
        healthy: accessible.filter((c) => c.health.status === 'healthy').length,
        degraded: accessible.filter((c) => c.health.status === 'degraded')
          .length,
        down: accessible.filter((c) => c.health.status === 'down').length,
        unknown: 0,
        queries_24h: entries.length,
        blocked_24h: entries.filter((row) => row.status === 'blocked').length,
        errors_24h: entries.filter((row) => row.status === 'error').length,
        pending_approvals: state.approvals.filter(
          (a) =>
            a.status === 'pending' &&
            accessible.some((c) => c.id === a.cluster_id),
        ).length,
        recent_queries: entries.slice(0, 10),
        unhealthy_clusters: accessible.filter(
          (c) => c.health.status !== 'healthy',
        ),
      };
    }
    if (resource === 'users') {
      if (id === 'lookup' && method === 'GET') {
        requireScopeAdmin();
        const q = (url.searchParams.get('q') ?? '').trim();
        validateText(q);
        if ([...q].length < 2 || [...q].length > 254)
          fail('validation', 'Search must contain 2..=254 characters');
        return {
          items: state.users
            .filter(
              (user) =>
                !user.disabled &&
                `${user.name} ${user.email}`
                  .toLowerCase()
                  .includes(q.toLowerCase()),
            )
            .sort(
              (a, b) =>
                a.name.toLowerCase().localeCompare(b.name.toLowerCase()) ||
                a.email.localeCompare(b.email),
            )
            .slice(0, 20)
            .map(({ id, email, name }) => ({ id, email, name })),
        };
      }
      admin();
      if (method === 'GET') return { items: state.users };
      if (method === 'POST') {
        if (state.users.some((user) => user.email === body.email))
          fail('conflict', 'Email already exists.', 409);
        const user: User = {
          id: crypto.randomUUID(),
          email: String(body.email),
          name: String(body.name),
          org_role: body.org_role === 'admin' ? 'admin' : 'member',
          disabled: false,
          created_at: now(),
          last_login_at: null,
        };
        if (String(body.password ?? '').length < 12)
          fail('validation', 'Use at least 12 characters.');
        state.users.push(user);
        passwords.set(user.id, String(body.password));
        audit('user.create', 'user', user.id, { email: user.email });
        return user;
      }
      const user = required(state.users.find((user) => user.id === id));
      if (method === 'DELETE') {
        user.disabled = true;
        audit('user.disable', 'user', user.id, {});
        return undefined;
      }
      if (body.password) {
        passwords.set(user.id, String(body.password));
        delete body.password;
      }
      Object.assign(user, body);
      return user;
    }
    if (resource === 'projects') {
      if (method === 'GET')
        return {
          items: state.projects
            .filter(
              (project) =>
                session?.org_role === 'admin' ||
                state.clusters.some(
                  (cluster) =>
                    cluster.project_id === project.id && clusterAccess(cluster),
                ),
            )
            .map((project) => ({
              ...project,
              cluster_count: state.clusters.filter(
                (c) => c.project_id === project.id,
              ).length,
            })),
        };
      admin();
      if (method === 'POST') {
        const project = {
          id: crypto.randomUUID(),
          name: String(body.name),
          description: String(body.description ?? ''),
          cluster_count: 0,
          created_at: now(),
        };
        state.projects.push(project);
        return project;
      }
      const project = required(
        state.projects.find((project) => project.id === id),
      );
      if (method === 'DELETE') {
        if (state.clusters.some((c) => c.project_id === id))
          fail('conflict', 'Remove this project’s clusters first.', 409);
        state.projects = state.projects.filter((p) => p.id !== id);
        return undefined;
      }
      Object.assign(project, body);
      return project;
    }
    if (resource === 'queries' && action === 'cancel') {
      const owner = running.get(id ?? '');
      if (owner && owner !== actor.id && actor.org_role !== 'admin')
        fail('forbidden', 'Only your own queries can be cancelled.', 403);
      cancelled.add(id ?? '');
      return undefined;
    }
    if (resource === 'clusters') {
      if (id === 'test-connection') {
        admin();
        if (body.password === undefined && body.cluster_id) {
          const stored = required(
            state.clusters.find((c) => c.id === body.cluster_id),
          );
          if (endpointChanged(stored, body as Partial<Cluster>))
            fail(
              'validation',
              'Changing the connection endpoint requires re-entering the password',
            );
        }
        if (!body.host || String(body.host).includes('invalid'))
          fail('upstream', 'Could not resolve database host.', 502);
        return {
          ...sampleCluster.health,
          status: 'healthy',
          latency_ms: 18,
          is_replica: false,
          checked_at: now(),
        };
      }
      if (!id) {
        if (method === 'GET')
          return {
            items: state.clusters
              .filter(
                (c) =>
                  clusterAccess(c) &&
                  (!url.searchParams.get('project_id') ||
                    c.project_id === url.searchParams.get('project_id')) &&
                  (!url.searchParams.get('environment') ||
                    c.environment === url.searchParams.get('environment')) &&
                  (!url.searchParams.get('engine') ||
                    c.engine === url.searchParams.get('engine')) &&
                  (!url.searchParams.get('q') ||
                    c.name.includes(url.searchParams.get('q') ?? '')),
              )
              .map((c) => ({ ...c, my_access: clusterAccess(c) })),
          };
        admin();
        const { password: secret, ...fields } = body;
        void secret;
        const cluster = {
          ...fields,
          id: crypto.randomUUID(),
          tags: body.tags ?? {},
          replica_host: body.replica_host ?? null,
          replica_port: body.replica_port ?? null,
          health: {
            ...sampleCluster.health,
            status: 'unknown',
            latency_ms: null,
            checked_at: null,
          },
          my_access: 'admin',
          created_at: now(),
          updated_at: now(),
        } as Cluster;
        required(state.projects.find((p) => p.id === cluster.project_id));
        state.clusters.push(cluster);
        policies.set(cluster.id, defaultPolicy(cluster.environment));
        audit('cluster.create', 'cluster', cluster.id, { name: cluster.name });
        return cluster;
      }
      const cluster = required(state.clusters.find((c) => c.id === id));
      if (!clusterAccess(cluster))
        fail('forbidden', 'You do not have access to this cluster.', 403);
      if (!action) {
        if (method === 'GET')
          return { ...cluster, my_access: clusterAccess(cluster) };
        if (method === 'DELETE') {
          admin();
          state.clusters = state.clusters.filter((c) => c.id !== id);
          state.grants = state.grants.filter((g) => g.scope_id !== id);
          audit('cluster.delete', 'cluster', cluster.id, {});
          return undefined;
        }
        clusterAdmin(cluster);
        if (
          endpointChanged(cluster, body as Partial<Cluster>) &&
          body.password === undefined
        )
          fail(
            'validation',
            'Changing the connection endpoint requires re-entering the password',
          );
        const { password: secret, ...fields } = body;
        void secret;
        Object.assign(cluster, fields, { updated_at: now() });
        audit('cluster.update', 'cluster', cluster.id, Object.keys(fields));
        return cluster;
      }
      const policy = required(policies.get(cluster.id));
      if (action === 'schema')
        return {
          ...demoSchema,
          schemas: demoSchema.schemas.map((schema, index) => ({
            ...schema,
            name:
              cluster.engine === 'mysql' && index === 0
                ? cluster.database
                : schema.name,
          })),
        };
      if (action === 'policy') {
        if (method === 'GET') return policy;
        clusterAdmin(cluster);
        if (
          Number(body.max_rows) < 1 ||
          Number(body.max_concurrent_queries) < 1
        )
          fail('validation', 'Limits must be positive.');
        policies.set(cluster.id, body as unknown as Policy);
        audit('policy.update', 'cluster', cluster.id, body);
        return body;
      }
      if (action === 'grants') {
        clusterAdmin(cluster);
        return {
          items: state.grants.filter(
            (g) => g.scope_id === id || g.scope_id === cluster.project_id,
          ),
        };
      }
      if (action === 'health') {
        if (method === 'POST') {
          clusterAdmin(cluster);
          cluster.health.checked_at = now();
          return cluster.health;
        }
        return {
          current: cluster.health,
          history: Array.from({ length: 24 }, (_, i) => ({
            status: cluster.health.status,
            latency_ms:
              cluster.health.latency_ms === null
                ? null
                : Math.max(1, cluster.health.latency_ms + Math.sin(i) * 12),
            checked_at: new Date(Date.now() - (23 - i) * 3600000).toISOString(),
          })),
        };
      }
      const sql = String(body.sql ?? '');
      const analysis = analyzeDemo(sql, policy);
      if (action === 'analyze') return analysis;
      if (action === 'query') {
        if (analysis.verdict === 'deny') {
          history(
            actor,
            cluster,
            sql,
            'blocked',
            analysis,
            undefined,
            'Query blocked by safety policy.',
          );
          fail(
            'query_denied',
            analysis.issues.find((issue) => issue.severity === 'block')
              ?.message ?? 'Query blocked.',
            422,
            analysis,
          );
        }
        if (analysis.verdict === 'requires_approval')
          fail(
            'approval_required',
            'This query needs approval.',
            409,
            analysis,
          );
        const query_id = String(body.query_id ?? crypto.randomUUID());
        running.set(query_id, actor.id);
        await wait(Math.max(delay, 750));
        running.delete(query_id);
        if (cancelled.delete(query_id)) {
          history(actor, cluster, sql, 'cancelled', analysis);
          fail('cancelled', 'Query was cancelled.', 409);
        }
        if (cluster.health.status === 'down') {
          history(
            actor,
            cluster,
            sql,
            'error',
            analysis,
            undefined,
            cluster.health.error,
          );
          fail(
            'upstream',
            cluster.health.error ?? 'Database is unavailable.',
            502,
          );
        }
        const value = result(cluster, sql, query_id);
        history(actor, cluster, sql, 'ok', analysis, value);
        return value;
      }
    }
    if (resource === 'history') {
      return page(
        state.history.filter(
          (row) =>
            (session?.org_role === 'admin' || row.user_id === session?.id) &&
            (!url.searchParams.get('cluster_id') ||
              row.cluster_id === url.searchParams.get('cluster_id')) &&
            (!url.searchParams.get('user_id') ||
              row.user_id === url.searchParams.get('user_id')) &&
            (!url.searchParams.get('status') ||
              row.status === url.searchParams.get('status')),
        ),
        url.searchParams,
      );
    }
    if (resource === 'grants') {
      if (method === 'GET' && !id) {
        requireScopeAdmin();
        const scope = url.searchParams.get('scope');
        if (scope && !['project', 'cluster'].includes(scope))
          fail('validation', 'Invalid grant scope');
        return page(
          state.grants.filter(
            (g) =>
              canAdminScope(g.scope, g.scope_id) &&
              (!scope || scope === g.scope) &&
              (!url.searchParams.get('scope_id') ||
                url.searchParams.get('scope_id') === g.scope_id) &&
              (!url.searchParams.get('user_id') ||
                url.searchParams.get('user_id') === g.user.id),
          ),
          url.searchParams,
        );
      }
      if (method === 'POST') {
        if (body.scope === 'cluster')
          clusterAdmin(
            required(state.clusters.find((c) => c.id === body.scope_id)),
          );
        else if (!canAdminScope('project', String(body.scope_id)))
          fail('forbidden', 'Project administrator access is required.', 403);
        const user = required(
          state.users.find((user) => user.id === body.user_id),
        );
        const scope_name =
          body.scope === 'cluster'
            ? state.clusters.find((c) => c.id === body.scope_id)?.name
            : state.projects.find((p) => p.id === body.scope_id)?.name;
        const grant = {
          id: crypto.randomUUID(),
          user: { id: user.id, email: user.email, name: user.name },
          scope: body.scope as 'project' | 'cluster',
          scope_id: String(body.scope_id),
          scope_name: required(scope_name),
          level: body.level as 'read' | 'write' | 'admin',
          expires_at: body.expires_at ? String(body.expires_at) : null,
          created_at: now(),
          created_by: actor.id,
        };
        state.grants.push(grant);
        audit('grant.create', 'grant', grant.id, {
          scope: grant.scope,
          level: grant.level,
        });
        return grant;
      }
      const grant = required(state.grants.find((g) => g.id === id));
      if (grant.scope === 'cluster')
        clusterAdmin(
          required(state.clusters.find((c) => c.id === grant.scope_id)),
        );
      else if (!canAdminScope(grant.scope, grant.scope_id))
        fail('forbidden', 'Project administrator access is required.', 403);
      state.grants = state.grants.filter((g) => g.id !== id);
      audit('grant.delete', 'grant', grant.id, {});
      return undefined;
    }
    if (resource === 'approvals') {
      if (!id && method === 'GET') {
        const listed = page(
          state.approvals.filter(
            (a) =>
              state.clusters.some(
                (c) =>
                  c.id === a.cluster_id &&
                  (a.requester.id === session?.id ||
                    clusterAccess(c) === 'admin'),
              ) &&
              (!url.searchParams.get('status') ||
                a.status === url.searchParams.get('status')) &&
              (!url.searchParams.get('cluster_id') ||
                a.cluster_id === url.searchParams.get('cluster_id')) &&
              (url.searchParams.get('mine') !== 'true' ||
                a.requester.id === session?.id),
          ),
          url.searchParams,
          100,
        );
        return {
          ...listed,
          items: listed.items.map((approval) => ({
            ...approval,
            sql: [...approval.sql].slice(0, 2000).join(''),
            sql_truncated: [...approval.sql].length > 2000,
            result: null,
          })),
        };
      }
      if (!id) {
        const cluster = required(
          state.clusters.find((c) => c.id === body.cluster_id),
        );
        const level = clusterAccess(cluster);
        if (level !== 'write' && level !== 'admin')
          fail('forbidden', 'Write access is required.', 403);
        const analysis = analyzeDemo(
          String(body.sql),
          required(policies.get(cluster.id)),
        );
        if (analysis.verdict === 'deny')
          fail('query_denied', 'This SQL is blocked.', 422, analysis);
        if (!String(body.reason ?? '').trim())
          fail('validation', 'A reason is required.');
        const approval: Approval = {
          id: crypto.randomUUID(),
          cluster_id: cluster.id,
          cluster_name: cluster.name,
          requester: { id: actor.id, email: actor.email, name: actor.name },
          sql: String(body.sql),
          reason: String(body.reason),
          analysis,
          status: 'pending',
          error: null,
          sql_truncated: false,
          reviewer: null,
          review_note: null,
          result: null,
          created_at: new Date(
            Math.max(
              Date.now(),
              ...state.approvals.map((a) => Date.parse(a.created_at) + 1),
            ),
          ).toISOString(),
          reviewed_at: null,
          executed_at: null,
          expires_at: new Date(Date.now() + 86400000).toISOString(),
        };
        state.approvals.unshift(approval);
        audit('approval.create', 'approval', approval.id, {});
        return approval;
      }
      const approval = required(state.approvals.find((a) => a.id === id));
      const cluster = required(
        state.clusters.find((c) => c.id === approval.cluster_id),
      );
      if (
        !clusterAccess(cluster) ||
        (approval.requester.id !== actor.id &&
          clusterAccess(cluster) !== 'admin')
      )
        fail('forbidden', 'No access to this request.', 403);
      if (!action) return approval;
      if (
        ['pending', 'approved'].includes(approval.status) &&
        Date.parse(approval.expires_at) < Date.now()
      ) {
        approval.status = 'expired';
        fail('conflict', 'This approval has expired.', 409);
      }
      if (action === 'approve' || action === 'reject') {
        clusterAdmin(cluster);
        if (approval.requester.id === actor.id)
          fail('forbidden', 'You cannot review your own request.', 403);
        if (approval.status !== 'pending')
          fail('conflict', 'This request has already been reviewed.', 409);
        if (action === 'reject' && !String(body.note ?? '').trim())
          fail('validation', 'A rejection note is required.');
        approval.status = action === 'approve' ? 'approved' : 'rejected';
        approval.reviewer = {
          id: actor.id,
          email: actor.email,
          name: actor.name,
        };
        approval.review_note = String(body.note ?? '');
        approval.reviewed_at = now();
      }
      if (action === 'execute') {
        if (
          approval.requester.id !== actor.id &&
          approval.reviewer?.id !== actor.id
        )
          fail('forbidden', 'Only the requester or reviewer may execute.', 403);
        if (approval.status !== 'approved')
          fail('conflict', 'Only an approved request can execute once.', 409);
        const analysis = analyzeDemo(
          approval.sql,
          required(policies.get(cluster.id)),
        );
        if (analysis.verdict === 'deny')
          fail(
            'query_denied',
            'Current policy blocks this SQL.',
            422,
            analysis,
          );
        approval.status = 'executing';
        await wait(delay);
        if (cluster.health.status === 'down') {
          approval.status = 'failed';
          approval.error = 'Target database unavailable';
          approval.executed_at = now();
          history(
            actor,
            cluster,
            approval.sql,
            'error',
            analysis,
            undefined,
            approval.error,
          );
          fail('upstream', approval.error, 502);
        }
        approval.result = result(cluster, approval.sql, crypto.randomUUID());
        approval.status = 'executed';
        approval.executed_at = now();
        history(
          actor,
          cluster,
          approval.sql,
          'ok',
          approval.analysis,
          approval.result,
        );
      }
      audit(
        `approval.${action === 'execute' ? 'executed' : action}`,
        'approval',
        approval.id,
        {
          note: approval.review_note,
        },
      );
      return approval;
    }
    if (resource === 'audit') {
      admin();
      return page(
        state.audit.filter(
          (event) =>
            (!url.searchParams.get('action') ||
              event.action === url.searchParams.get('action')) &&
            (!url.searchParams.get('actor_id') ||
              event.actor?.id === url.searchParams.get('actor_id')),
        ),
        url.searchParams,
      );
    }
    if (resource === 'settings' && id === 'network') {
      admin();
      if (method === 'PUT') {
        const cidrs = body.allowed_cidrs as string[];
        if (!Array.isArray(cidrs) || cidrs.some((cidr) => !isCidr(cidr)))
          fail('validation', 'Enter valid IPv4 or IPv6 CIDRs.');
        if (
          cidrs.length &&
          !cidrs.some(
            (cidr) =>
              cidr === '10.0.0.0/8' ||
              cidr === '10.12.0.0/16' ||
              cidr === '10.12.0.24/32' ||
              cidr === '0.0.0.0/0',
          )
        )
          fail(
            'validation',
            'This change would lock out your demo IP (10.12.0.24).',
          );
        network = {
          allowed_cidrs: cidrs,
          trust_proxy_headers: body.trust_proxy_headers === true,
        };
        audit('network.updated', 'settings', 'network', network);
      }
      return network;
    }
    fail('not_found', 'Endpoint not found.', 404);
  }
  return async <T>(path: string, init?: RequestInit) => {
    if (init?.signal?.aborted) throw new DOMException('Aborted', 'AbortError');
    await wait(delay);
    const value = await route(path, init);
    if (init?.signal?.aborted) throw new DOMException('Aborted', 'AbortError');
    return structuredClone(value) as T;
  };
}
