import { ApiError } from './errors';
import { discoveryFixtures } from './discovery-fixtures';
import { now } from './fixtures';
import { suggestEnvironment } from '../features/discovery/helpers';
import type {
  Cluster,
  DiscoveryCounts,
  DiscoverySource,
  DiscoveredResource,
  Drift,
  Project,
  ScanRun,
  DiscoveryTest,
} from './types';

const fail = (code: string, message: string, status = 400): never => {
  throw new ApiError(code, message, status);
};
const required = <T>(value: T | undefined): T =>
  value ?? fail('not_found', 'Discovery item no longer exists.', 404);
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
function paginate<T extends { id: string }>(
  items: T[],
  query: URLSearchParams,
) {
  const limit = Number(query.get('limit') ?? 50);
  if (!Number.isInteger(limit) || limit < 1 || limit > 200)
    fail('validation', 'Limit must be 1..=200');
  const orderKey = (item: T) =>
    `${'started_at' in item ? item.started_at : 'first_seen_at' in item ? item.first_seen_at : ''}|${item.id}`;
  const sorted = [...items].sort((a, b) =>
    orderKey(b).localeCompare(orderKey(a)),
  );
  const cursor = query.get('cursor');
  let boundary: string | undefined;
  if (cursor) {
    try {
      boundary = atob(cursor.replaceAll('-', '+').replaceAll('_', '/'));
    } catch {
      fail('validation', 'Invalid cursor');
    }
    if (!boundary || !/^\d{4}-\d{2}-\d{2}T[^|]+\|[a-f\d-]{36}$/i.test(boundary))
      fail('validation', 'Invalid cursor');
  }
  const remaining = boundary
    ? sorted.filter((item) => orderKey(item) < boundary)
    : sorted;
  const batch = remaining.slice(0, limit);
  return {
    items: batch,
    next_cursor:
      remaining.length > limit
        ? btoa(orderKey(required(batch.at(-1))))
            .replaceAll('+', '-')
            .replaceAll('/', '_')
            .replace(/=+$/, '')
        : null,
  };
}
function validateSource(
  source: DiscoverySource,
  body: Record<string, unknown>,
  projects: Project[],
) {
  if (
    typeof source.name !== 'string' ||
    !source.name.trim() ||
    source.provider !== 'aws'
  )
    fail('validation', 'A name and AWS provider are required.');
  if (typeof source.enabled !== 'boolean')
    fail('validation', 'Enabled must be a boolean.');
  if (
    source.role_arn &&
    !/^arn:aws:iam::\d{12}:role\/[\w+=,.@/-]+$/.test(source.role_arn)
  )
    fail('validation', 'Enter a commercial AWS IAM role ARN.');
  if (
    !Array.isArray(source.regions) ||
    source.regions.length < 1 ||
    source.regions.length > 20 ||
    source.regions.some(
      (r) =>
        typeof r !== 'string' ||
        !/^(us|eu|ap|ca|sa|me|af|il|mx)-[a-z]+-\d$/.test(r),
    )
  )
    fail('validation', 'Choose 1–20 commercial AWS regions.');
  if (
    !Number.isInteger(source.scan_interval_minutes) ||
    source.scan_interval_minutes < 5 ||
    source.scan_interval_minutes > 1440
  )
    fail('validation', 'Scan interval must be 5–1440 minutes.');
  if (
    !Array.isArray(source.environment_tag_keys) ||
    source.environment_tag_keys.some(
      (key) => typeof key !== 'string' || !key.trim(),
    )
  )
    fail('validation', 'Enter valid environment tag keys.');
  if (
    body.external_id !== undefined &&
    body.external_id !== null &&
    (typeof body.external_id !== 'string' || !body.external_id.trim())
  )
    fail('validation', 'External ID cannot be empty. Use null to clear it.');
  if (source.default_project_id)
    required(projects.find((p) => p.id === source.default_project_id));
}
function resourceDrift(
  resource: DiscoveredResource,
  cluster: Cluster,
): Drift[] {
  const drift: Drift[] = [];
  if (cluster.host !== resource.host || cluster.port !== resource.port)
    drift.push('endpoint_changed');
  if (
    cluster.replica_host !== resource.replica_host ||
    cluster.replica_port !== resource.replica_port
  )
    drift.push('replica_changed');
  if (cluster.engine !== resource.engine) drift.push('engine_changed');
  if (resource.status === 'gone') drift.push('deleted');
  return drift;
}
/** Mock discovery has its own inventory; cluster creation uses the existing mock's policy path. */
export function createMockDiscovery({
  clusters,
  projects,
  addCluster,
  delay,
}: {
  clusters: () => Cluster[];
  projects: () => Project[];
  addCluster: (body: Record<string, unknown>) => Cluster;
  delay: number;
}) {
  const state = discoveryFixtures(clusters());
  const inventory = structuredClone(
    state.resources.filter((r) => r.status !== 'gone'),
  );
  const draftTests = new Map<string, DiscoverySource['last_test']>();
  const externalIds = new Map<string, string | null>();
  const testKey = (source: DiscoverySource, external: unknown) =>
    JSON.stringify([source.role_arn, external ?? null, source.regions]);
  const summary = (report: DiscoveryTest): DiscoverySource['last_test'] => ({
    ok: report.ok,
    account_id: report.account_id,
    identity_arn: report.identity_arn,
    tested_at: now(),
  });
  const tests = (source: DiscoverySource) => {
    const denied = source.role_arn?.includes('Denied') ?? false;
    const account_id = denied
      ? null
      : (source.role_arn?.split(':')[4] ?? '111222333444');
    const regions = source.regions.map((region) => ({
      region,
      ok: !denied && region !== 'ap-east-1',
      error: denied
        ? 'Access denied when assuming this role.'
        : region === 'ap-east-1'
          ? 'Region is not enabled in this account.'
          : null,
    }));
    return {
      ok: regions.every((r) => r.ok),
      account_id,
      identity_arn: account_id
        ? `arn:aws:sts::${account_id}:assumed-role/${source.role_arn?.split('/').at(-1) ?? 'Gateway'}/visp-db-access`
        : null,
      regions,
    };
  };
  async function scan(source: DiscoverySource) {
    if (source.last_scan?.status === 'running')
      fail('conflict', 'A scan is already running for this source.', 409);
    const run: ScanRun = {
      id: crypto.randomUUID(),
      source_id: source.id,
      status: 'running',
      started_at: now(),
      finished_at: null,
      found: 0,
      new: 0,
      gone: 0,
      changed: 0,
      errors: [],
    };
    state.runs.unshift(run);
    source.last_scan = run;
    await wait(Math.max(delay, 30));
    const test = tests(source);
    run.errors = test.regions
      .filter((r) => !r.ok)
      .map((r) => ({ region: r.region, message: r.error ?? 'Scan failed.' }));
    const good = new Set(test.regions.filter((r) => r.ok).map((r) => r.region));
    const candidates = inventory
      .filter((r) => good.has(r.region))
      .map((r) => ({
        ...r,
        suggested_environment: suggestEnvironment(
          r.tags,
          r.identifier,
          source.environment_tag_keys,
        ),
        account_id: test.account_id ?? r.account_id,
        arn: r.arn.replace(
          `:${r.account_id}:`,
          `:${test.account_id ?? r.account_id}:`,
        ),
      }));
    for (const candidate of candidates) {
      const existing = state.resources.find(
        (r) => r.source_id === source.id && r.arn === candidate.arn,
      );
      if (existing) {
        Object.assign(existing, {
          ...candidate,
          id: existing.id,
          source_id: source.id,
          source_name: source.name,
          first_seen_at: existing.first_seen_at,
          last_seen_at: now(),
          status:
            existing.status === 'gone'
              ? existing.cluster_id
                ? 'imported'
                : 'new'
              : existing.status,
          cluster_id: existing.cluster_id,
        });
        if (existing.cluster_id)
          existing.drift = resourceDrift(
            existing,
            required(clusters().find((c) => c.id === existing.cluster_id)),
          );
        else existing.drift = [];
      } else {
        state.resources.push({
          ...candidate,
          id: crypto.randomUUID(),
          source_id: source.id,
          source_name: source.name,
          status: 'new',
          cluster_id: null,
          drift: [],
          first_seen_at: now(),
          last_seen_at: now(),
        });
        run.new++;
      }
    }
    const seen = new Set(candidates.map((r) => r.arn));
    for (const resource of state.resources.filter(
      (r) => r.source_id === source.id && good.has(r.region),
    )) {
      if (!seen.has(resource.arn) && resource.status !== 'gone') {
        resource.status = 'gone';
        run.gone++;
        if (resource.cluster_id) resource.drift = ['deleted'];
      }
    }
    run.found = candidates.length;
    run.changed = state.resources.filter(
      (r) => r.source_id === source.id && r.drift.length && r.status !== 'gone',
    ).length;
    run.status = run.errors.length
      ? good.size
        ? 'partial'
        : 'failed'
      : 'succeeded';
    run.finished_at = now();
    return run;
  }
  function sourceFields(body: Record<string, unknown>) {
    const fields: Record<string, unknown> = {};
    for (const key of [
      'provider',
      'name',
      'role_arn',
      'regions',
      'default_project_id',
      'environment_tag_keys',
      'scan_interval_minutes',
      'enabled',
    ])
      if (body[key] !== undefined) fields[key] = body[key];
    return fields;
  }
  function importResource(
    resource: DiscoveredResource,
    body: Record<string, unknown>,
  ) {
    if (resource.cluster_id)
      fail('conflict', 'This resource is already imported.', 409);
    if (resource.status !== 'new')
      fail('conflict', 'Only new resources can be imported.', 409);
    for (const key of [
      'project_id',
      'name',
      'database',
      'username',
      'password',
    ])
      if (!String(body[key] ?? '').trim())
        fail('validation', `${key} is required.`);
    if (
      !['production', 'staging', 'development'].includes(
        String(body.environment),
      )
    )
      fail('validation', 'Choose an environment.');
    if (
      !['disable', 'prefer', 'require', 'verify_full'].includes(
        String(body.tls_mode),
      )
    )
      fail('validation', 'Choose a TLS mode.');
    required(projects().find((p) => p.id === body.project_id));
    const cluster = addCluster({
      project_id: body.project_id,
      name: body.name,
      environment: body.environment,
      database: body.database,
      username: body.username,
      password: body.password,
      tls_mode: body.tls_mode,
      host: resource.host,
      port: resource.port,
      engine: resource.engine,
      provider: resource.provider,
      region: resource.region,
      replica_host: resource.replica_host,
      replica_port: resource.replica_port,
      tags: { ...resource.tags, 'discovery:arn': resource.arn },
    });
    resource.status = 'imported';
    resource.cluster_id = cluster.id;
    resource.drift = [];
    return cluster;
  }
  return {
    summary: () => ({
      new: state.resources.filter((r) => r.status === 'new').length,
      gone: state.resources.filter((r) => r.status === 'gone').length,
      drifted: state.resources.filter((r) => r.cluster_id && r.drift.length)
        .length,
    }),
    route: async (
      url: URL,
      method: string,
      body: Record<string, unknown>,
    ): Promise<unknown> => {
      const [, category, id, action] = url.pathname.split('/').filter(Boolean);
      if (category === 'sources') {
        if (!id && method === 'GET') return { items: state.sources };
        if ((!id || id === 'test') && method === 'POST') {
          const source = {
            id: crypto.randomUUID(),
            provider: 'aws',
            name: '',
            role_arn: null,
            regions: [],
            default_project_id: null,
            environment_tag_keys: ['environment', 'env', 'stage'],
            scan_interval_minutes: 60,
            enabled: true,
            external_id_set: body.external_id != null,
            last_scan: null,
            last_test: null,
            created_at: now(),
            updated_at: now(),
            ...sourceFields(body),
          } as DiscoverySource;
          validateSource(source, body, projects());
          if (id === 'test') {
            const report = tests(source);
            draftTests.set(testKey(source, body.external_id), summary(report));
            return report;
          }
          externalIds.set(
            source.id,
            (body.external_id as string | null) ?? null,
          );
          source.last_test =
            draftTests.get(testKey(source, body.external_id)) ?? null;
          state.sources.push(source);
          return source;
        }
        const source = required(state.sources.find((s) => s.id === id));
        if (!action && method === 'PATCH') {
          const draft = { ...source, ...sourceFields(body), updated_at: now() };
          validateSource(draft, body, projects());
          if (body.external_id !== undefined)
            draft.external_id_set = body.external_id !== null;
          const oldExternal = externalIds.get(source.id) ?? null;
          const newExternal =
            body.external_id === undefined ? oldExternal : body.external_id;
          if (testKey(source, oldExternal) !== testKey(draft, newExternal))
            draft.last_test =
              draftTests.get(testKey(draft, newExternal)) ?? null;
          externalIds.set(source.id, newExternal as string | null);
          Object.assign(source, draft);
          state.resources
            .filter((r) => r.source_id === id)
            .forEach((r) => {
              r.source_name = source.name;
            });
          return source;
        }
        if (!action && method === 'DELETE') {
          if (source.last_scan?.status === 'running')
            fail('conflict', 'Wait for the current scan to finish.', 409);
          state.sources = state.sources.filter((s) => s.id !== id);
          state.resources = state.resources.filter((r) => r.source_id !== id);
          state.runs = state.runs.filter((r) => r.source_id !== id);
          return undefined;
        }
        if (action === 'test' && method === 'POST') {
          const report = tests(source);
          source.last_test = summary(report);
          return report;
        }
        if (action === 'scan' && method === 'POST') return scan(source);
        if (action === 'runs' && method === 'GET')
          return paginate(
            state.runs.filter((run) => run.source_id === id),
            url.searchParams,
          );
      }
      if (category === 'resources') {
        if (!id && method === 'GET') {
          const query = url.searchParams;
          const filtered = state.resources.filter(
            (r) =>
              ['source_id', 'engine', 'region'].every(
                (key) =>
                  !query.get(key) ||
                  r[key as 'source_id' | 'engine' | 'region'] ===
                    query.get(key),
              ) &&
              (!query.get('q') ||
                `${r.identifier} ${r.host} ${r.arn} ${Object.values(r.tags).join(' ')}`
                  .toLowerCase()
                  .includes((query.get('q') ?? '').trim().toLowerCase())),
          );
          const counts: DiscoveryCounts = {
            new: 0,
            imported: 0,
            ignored: 0,
            gone: 0,
          };
          filtered.forEach((r) => {
            counts[r.status]++;
          });
          return {
            ...paginate(
              filtered.filter(
                (r) => !query.get('status') || r.status === query.get('status'),
              ),
              query,
            ),
            counts,
          };
        }
        const resource = required(state.resources.find((r) => r.id === id));
        if (method !== 'POST') fail('not_found', 'Endpoint not found.', 404);
        if (action === 'import') return importResource(resource, body);
        if (action === 'ignore' || action === 'unignore') {
          if (resource.cluster_id || resource.status === 'gone')
            fail(
              'conflict',
              'Imported or gone resources cannot be ignored.',
              409,
            );
          resource.status = action === 'ignore' ? 'ignored' : 'new';
          return resource;
        }
        if (action === 'sync') {
          const cluster = required(
            clusters().find((c) => c.id === resource.cluster_id),
          );
          if (resource.status === 'gone')
            fail('conflict', 'This database no longer exists in AWS.', 409);
          const changes = resourceDrift(resource, cluster);
          if (
            (changes.includes('endpoint_changed') ||
              changes.includes('replica_changed')) &&
            !String(body.password ?? '').trim()
          )
            fail(
              'validation',
              'Changing the connection endpoint requires re-entering the password',
            );
          Object.assign(cluster, {
            host: resource.host,
            port: resource.port,
            replica_host: resource.replica_host,
            replica_port: resource.replica_port,
            updated_at: now(),
          });
          resource.drift = resourceDrift(resource, cluster);
          return cluster;
        }
      }
      fail('not_found', 'Endpoint not found.', 404);
    },
  };
}
