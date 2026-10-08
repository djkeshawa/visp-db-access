import { describe, it, expect } from 'vitest';
import { createMockTransport } from '../src/api/mock';
import type {
  Analysis,
  Approval,
  Cluster,
  QueryResult,
  User,
} from '../src/api/types';
const post = (body: unknown): RequestInit => ({
  method: 'POST',
  body: JSON.stringify(body),
});
async function setup() {
  const api = createMockTransport({ delay: 0 });
  const { user } = await api<{ user: User }>(
    '/auth/login',
    post({ email: 'admin@visp.dev', password: 'demo-password' }),
  );
  const { items } = await api<{ items: Cluster[] }>('/clusters');
  return { api, user, cluster: items[0]! };
}
describe('demo API safety and lifecycle', () => {
  it('requires a session and rejects invalid credentials', async () => {
    const api = createMockTransport({ delay: 0 });
    await expect(api('/clusters')).rejects.toMatchObject({ status: 401 });
    await expect(
      api('/auth/login', post({ email: 'admin@visp.dev', password: 'wrong' })),
    ).rejects.toMatchObject({ status: 401 });
  });
  it('enforces a read limit and rejects unsafe writes, sleep, CTE writes and multiple statements', async () => {
    const { api, cluster } = await setup();
    const analyze = (sql: string) =>
      api<Analysis>(`/clusters/${cluster.id}/analyze`, post({ sql }));
    expect(await analyze('SELECT * FROM users')).toMatchObject({
      verdict: 'allow',
      rewritten_sql: expect.stringContaining('LIMIT'),
    });
    for (const sql of [
      'DELETE FROM users',
      "UPDATE users SET name = 'x'",
      'SELECT pg_sleep(60)',
      'WITH removed AS (DELETE FROM users RETURNING *) SELECT * FROM removed',
      'SELECT 1; DELETE FROM users',
    ])
      expect(await analyze(sql)).toMatchObject({ verdict: 'deny' });
    expect(
      await analyze("UPDATE users SET name = 'x' WHERE id = 1"),
    ).toMatchObject({ verdict: 'requires_approval' });
  });
  it('cannot execute writes without approval and enforces four-eyes', async () => {
    const { api, cluster } = await setup();
    const sql = "UPDATE users SET name = 'x' WHERE id = 1";
    await expect(
      api(`/clusters/${cluster.id}/query`, post({ sql })),
    ).rejects.toMatchObject({ code: 'approval_required' });
    const approval = await api<Approval>(
      '/approvals',
      post({ cluster_id: cluster.id, sql, reason: 'Fix name' }),
    );
    await expect(
      api(`/approvals/${approval.id}/approve`, post({})),
    ).rejects.toMatchObject({ status: 403 });
  });
  it('allows another admin to approve, executes once, and retains the result', async () => {
    const { api, cluster } = await setup();
    const approval = await api<Approval>(
      '/approvals',
      post({
        cluster_id: cluster.id,
        sql: "UPDATE users SET name = 'x' WHERE id = 1",
        reason: 'Fix name',
      }),
    );
    await api(
      '/auth/login',
      post({ email: 'reviewer@visp.dev', password: 'demo-password' }),
    );
    await api(`/approvals/${approval.id}/approve`, post({ note: 'Reviewed' }));
    expect(
      await api<Approval>(`/approvals/${approval.id}/execute`, post({})),
    ).toMatchObject({ status: 'executed', result: { affected_rows: 1 } });
    await expect(
      api(`/approvals/${approval.id}/execute`, post({})),
    ).rejects.toMatchObject({ status: 409 });
  });
  it('cancels by client-generated query id and records blocked history', async () => {
    const { api, cluster } = await setup();
    const query_id = crypto.randomUUID();
    const pending = api<QueryResult>(
      `/clusters/${cluster.id}/query`,
      post({ sql: 'SELECT * FROM users', query_id }),
    );
    await api(`/queries/${query_id}/cancel`, post({}));
    await expect(pending).rejects.toMatchObject({ code: 'cancelled' });
    await expect(
      api(`/clusters/${cluster.id}/query`, post({ sql: 'DELETE FROM users' })),
    ).rejects.toMatchObject({ code: 'query_denied' });
    const history = await api<{ items: { status: string }[] }>(
      '/history?status=blocked',
    );
    expect(history.items.some((row) => row.status === 'blocked')).toBe(true);
  });
  it('does not expose connection passwords and prevents deleting a populated project', async () => {
    const { api, cluster } = await setup();
    expect(cluster).not.toHaveProperty('password');
    await expect(
      api(`/projects/${cluster.project_id}`, { method: 'DELETE' }),
    ).rejects.toMatchObject({ status: 409 });
  });
  it('restricts member administration and validates policy/network inputs', async () => {
    const { api, cluster } = await setup();
    await expect(
      api('/settings/network', {
        method: 'PUT',
        body: JSON.stringify({
          allowed_cidrs: ['bad'],
          trust_proxy_headers: false,
        }),
      }),
    ).rejects.toMatchObject({ status: 400 });
    await api(
      '/auth/login',
      post({ email: 'member@visp.dev', password: 'demo-password' }),
    );
    await expect(api('/users')).rejects.toMatchObject({ status: 403 });
    await expect(
      api(`/clusters/${cluster.id}/policy`, { method: 'PUT', body: '{}' }),
    ).rejects.toMatchObject({ status: 403 });
  });
});

describe('hardened administration and approval contract', () => {
  it('lists grants including projects without clusters and supports cursors and filters', async () => {
    const { api, user } = await setup();
    const project = await api<{ id: string }>(
      '/projects',
      post({ name: 'Empty', description: '' }),
    );
    const grant = await api<{ id: string }>(
      '/grants',
      post({
        user_id: user.id,
        scope: 'project',
        scope_id: project.id,
        level: 'read',
      }),
    );
    const filtered = await api<{
      items: { id: string }[];
      next_cursor: string | null;
    }>(`/grants?scope=project&scope_id=${project.id}&user_id=${user.id}`);
    expect(filtered.items.map((g) => g.id)).toEqual([grant.id]);
    expect(filtered.next_cursor).toBeNull();
    const first = await api<{ items: { id: string }[]; next_cursor: string }>(
      '/grants?limit=1',
    );
    const second = await api<{ items: { id: string }[] }>(
      `/grants?limit=1&cursor=${first.next_cursor}`,
    );
    expect(first.items[0]?.id).not.toBe(second.items[0]?.id);
    await expect(api('/grants?limit=0')).rejects.toMatchObject({
      code: 'validation',
      status: 400,
    });
  });
  it('supports scoped user lookup without granting full user administration', async () => {
    const { api, cluster } = await setup();
    const member = (await api<{ items: User[] }>('/users')).items.find(
      (u) => u.org_role === 'member',
    )!;
    await api(
      '/grants',
      post({
        user_id: member.id,
        scope: 'cluster',
        scope_id: cluster.id,
        level: 'admin',
      }),
    );
    await api(
      '/auth/login',
      post({ email: member.email, password: 'demo-password' }),
    );
    await expect(api('/users')).rejects.toMatchObject({ status: 403 });
    const found = await api<{
      items: { id: string; email: string; name: string }[];
    }>('/users/lookup?q=alex');
    expect(found.items).toHaveLength(1);
    expect(Object.keys(found.items[0]!)).toEqual(['id', 'email', 'name']);
    await expect(api('/users/lookup?q=a')).rejects.toMatchObject({
      status: 400,
    });
    const grants = await api<{ items: { scope_id: string }[] }>('/grants');
    expect(grants.items.every((g) => g.scope_id === cluster.id)).toBe(true);
  });
  it('paginates approval summaries, returns full details and does not leak result rows into lists', async () => {
    const { api, cluster } = await setup();
    const sql = `UPDATE users SET name = '${'x'.repeat(2100)}' WHERE id = 1`;
    const approval = await api<Approval>(
      '/approvals',
      post({ cluster_id: cluster.id, sql, reason: 'Long SQL' }),
    );
    const first = await api<{ items: Approval[]; next_cursor: string }>(
      '/approvals?limit=1',
    );
    expect(first.next_cursor).toBeTruthy();
    expect(first.items[0]).toMatchObject({
      id: approval.id,
      sql_truncated: true,
      result: null,
      error: null,
    });
    expect([...first.items[0]!.sql]).toHaveLength(2000);
    expect(await api<Approval>(`/approvals/${approval.id}`)).toMatchObject({
      sql,
      sql_truncated: false,
    });
    await expect(api('/approvals?limit=101')).rejects.toMatchObject({
      status: 400,
    });
  });
  it('requires a new password when changing any connection endpoint field and rejects control characters', async () => {
    const { api, cluster } = await setup();
    await expect(
      api(`/clusters/${cluster.id}`, {
        method: 'PATCH',
        body: JSON.stringify({ host: 'changed.example' }),
      }),
    ).rejects.toMatchObject({
      code: 'validation',
      message:
        'Changing the connection endpoint requires re-entering the password',
    });
    await expect(
      api(
        '/clusters/test-connection',
        post({
          engine: cluster.engine,
          host: 'changed.example',
          port: cluster.port,
          database: cluster.database,
          username: cluster.username,
          tls_mode: cluster.tls_mode,
          cluster_id: cluster.id,
        }),
      ),
    ).rejects.toMatchObject({ code: 'validation' });
    await expect(
      api('/projects', post({ name: 'bad\u0000', description: '' })),
    ).rejects.toMatchObject({ code: 'validation' });
  });
});
