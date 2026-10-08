import { describe, expect, it } from 'vitest';
import { createMockTransport } from '../src/api/mock';
import { json } from '../src/api/client';
import type {
  Cluster,
  DiscoverySource,
  ResourcePage,
  ScanRun,
} from '../src/api/types';
import {
  groupRegions,
  driftDiff,
  suggestEnvironment,
} from '../src/features/discovery/helpers';

async function setup() {
  const api = createMockTransport({ delay: 0 });
  await api(
    '/auth/login',
    json('POST', { email: 'admin@visp.dev', password: 'demo-password' }),
  );
  const sources = await api<{ items: DiscoverySource[] }>('/discovery/sources');
  return { api, source: sources.items[0]! };
}
const imported = {
  project_id: '00000000-0000-4000-8000-000000000010',
  name: 'Imported',
  environment: 'production',
  database: 'app',
  username: 'gateway',
  password: 'secret',
  tls_mode: 'verify_full',
};
describe('cloud discovery contract', () => {
  it('paginates resources without duplicates and counts exclude the status filter', async () => {
    const { api } = await setup();
    const first = await api<ResourcePage>(
      '/discovery/resources?limit=2&status=new',
    );
    const second = await api<ResourcePage>(
      `/discovery/resources?limit=2&status=new&cursor=${first.next_cursor}`,
    );
    expect(first.items).toHaveLength(2);
    expect(second.items).toHaveLength(2);
    expect(
      new Set([...first.items, ...second.items].map((r) => r.id)).size,
    ).toBe(4);
    expect(first.counts.imported).toBeGreaterThan(0);
    expect(first.counts.gone).toBe(1);
    await expect(api('/discovery/resources?limit=0')).rejects.toMatchObject({
      code: 'validation',
    });
  });
  it('keeps external IDs write-only, tests identity, and supports clearing', async () => {
    const { api, source } = await setup();
    const updated = await api<DiscoverySource>(
      `/discovery/sources/${source.id}`,
      json('PATCH', { external_id: 'private-secret' }),
    );
    expect(updated.external_id_set).toBe(true);
    expect(updated).not.toHaveProperty('external_id');
    expect(
      await api(`/discovery/sources/${source.id}/test`, json('POST')),
    ).toMatchObject({
      ok: true,
      account_id: expect.any(String),
      identity_arn: expect.any(String),
    });
    expect(
      await api(
        `/discovery/sources/${source.id}`,
        json('PATCH', { external_id: null }),
      ),
    ).toMatchObject({ external_id_set: false });
  });
  it('scans a new source, records history, and refuses overlapping scans', async () => {
    const { api } = await setup();
    const source = await api<DiscoverySource>(
      '/discovery/sources',
      json('POST', {
        provider: 'aws',
        name: 'New account',
        regions: ['eu-west-1'],
      }),
    );
    const pending = api<ScanRun>(
      `/discovery/sources/${source.id}/scan`,
      json('POST'),
    );
    await expect(
      api(`/discovery/sources/${source.id}/scan`, json('POST')),
    ).rejects.toMatchObject({ status: 409 });
    expect(await pending).toMatchObject({
      status: 'succeeded',
      found: 3,
      new: 3,
    });
    expect(await api(`/discovery/sources/${source.id}/runs`)).toMatchObject({
      items: [expect.objectContaining({ status: 'succeeded' })],
    });
    expect(
      await api<ScanRun>(`/discovery/sources/${source.id}/scan`, json('POST')),
    ).toMatchObject({ new: 0 });
  });
  it('ignores and unignores, imports once and retains clusters when deleting a source', async () => {
    const { api, source } = await setup();
    const resource = (
      await api<ResourcePage>(
        `/discovery/resources?source_id=${source.id}&status=new`,
      )
    ).items[0]!;
    expect(
      await api(`/discovery/resources/${resource.id}/ignore`, json('POST')),
    ).toMatchObject({ status: 'ignored' });
    expect(
      await api(`/discovery/resources/${resource.id}/unignore`, json('POST')),
    ).toMatchObject({ status: 'new' });
    const projects = await api<{ items: { id: string }[] }>('/projects');
    const body = { ...imported, project_id: projects.items[0]!.id };
    const cluster = await api<Cluster>(
      `/discovery/resources/${resource.id}/import`,
      json('POST', body),
    );
    expect(cluster).toMatchObject({
      host: resource.host,
      replica_host: resource.replica_host,
      engine: resource.engine,
      tls_mode: 'verify_full',
    });
    expect(cluster).not.toHaveProperty('password');
    await expect(
      api(`/discovery/resources/${resource.id}/import`, json('POST', body)),
    ).rejects.toMatchObject({ status: 409 });
    await api(`/discovery/sources/${source.id}`, json('DELETE'));
    expect(await api(`/clusters/${cluster.id}`)).toMatchObject({
      id: cluster.id,
    });
    expect(
      (await api<ResourcePage>(`/discovery/resources?source_id=${source.id}`))
        .items,
    ).toHaveLength(0);
  });
  it('requires password for endpoint sync and clears only applied drift', async () => {
    const { api } = await setup();
    const resource = (
      await api<ResourcePage>('/discovery/resources?status=imported')
    ).items[0]!;
    const current = await api<Cluster>(`/clusters/${resource.cluster_id}`);
    expect(driftDiff(current, resource).some((d) => d.field === 'host')).toBe(
      true,
    );
    await expect(
      api(`/discovery/resources/${resource.id}/sync`, json('POST', {})),
    ).rejects.toMatchObject({ code: 'validation' });
    expect(
      await api(
        `/discovery/resources/${resource.id}/sync`,
        json('POST', { password: 'fresh-secret' }),
      ),
    ).toMatchObject({ host: resource.host });
    expect(
      (await api<ResourcePage>('/discovery/resources?status=imported')).items[0]
        ?.drift,
    ).toEqual([]);
  });
  it('protects all discovery endpoints from members and hides overview counts', async () => {
    const { api, source } = await setup();
    await api(
      '/auth/login',
      json('POST', { email: 'member@visp.dev', password: 'demo-password' }),
    );
    for (const path of [
      '/discovery/sources',
      '/discovery/resources',
      `/discovery/sources/${source.id}/runs`,
    ])
      await expect(api(path)).rejects.toMatchObject({ status: 403 });
    expect(await api('/overview')).toMatchObject({ discovery: null });
  });
});
describe('discovery helpers', () => {
  it('searches region names and codes and groups commercial regions', () => {
    expect(groupRegions('Sydney')).toEqual([
      {
        geography: 'Asia Pacific',
        regions: [expect.objectContaining({ code: 'ap-southeast-2' })],
      },
    ]);
    expect(groupRegions('eu-west')).toHaveLength(1);
    expect(
      groupRegions()
        .flatMap((g) => g.regions)
        .some((r) => r.code.startsWith('cn-') || r.code.startsWith('us-gov')),
    ).toBe(false);
    expect(groupRegions('no matching region')).toEqual([]);
  });
});

// Tag precedence and name fallback keep unknown databases on production defaults.
describe('environment inference', () => {
  it('uses configured tag precedence, conservative name tokens, and production fallback', () => {
    expect(
      suggestEnvironment({ stage: 'staging', env: 'prod' }, 'orders-dev', [
        'env',
        'stage',
      ]),
    ).toBe('production');
    expect(
      suggestEnvironment({ stage: 'staging', env: 'prod' }, 'orders-dev', [
        'stage',
        'env',
      ]),
    ).toBe('staging');
    expect(suggestEnvironment({}, 'orders-dev', [])).toBe('development');
    expect(suggestEnvironment({}, 'device-service', [])).toBe('production');
    expect(
      suggestEnvironment({ environment: 'unrecognized' }, 'orders', [
        'environment',
      ]),
    ).toBe('production');
  });
});

describe('discovery edge cases', () => {
  it('reports partial/failed scans without marking inaccessible regions gone', async () => {
    const { api, source } = await setup();
    await api(
      `/discovery/sources/${source.id}`,
      json('PATCH', { regions: ['us-east-1', 'ap-east-1'] }),
    );
    expect(
      await api(`/discovery/sources/${source.id}/scan`, json('POST')),
    ).toMatchObject({
      status: 'partial',
      found: 3,
      errors: [{ region: 'ap-east-1', message: expect.any(String) }],
    });
    const before = await api<ResourcePage>(
      `/discovery/resources?source_id=${source.id}`,
    );
    await api(
      `/discovery/sources/${source.id}`,
      json('PATCH', { role_arn: 'arn:aws:iam::111222333444:role/Denied' }),
    );
    expect(
      await api(`/discovery/sources/${source.id}/scan`, json('POST')),
    ).toMatchObject({ status: 'failed', found: 0, gone: 0 });
    expect(
      (await api<ResourcePage>(`/discovery/resources?source_id=${source.id}`))
        .counts,
    ).toEqual(before.counts);
    const first = await api<{ items: ScanRun[]; next_cursor: string }>(
      `/discovery/sources/${source.id}/runs?limit=1`,
    );
    const second = await api<{ items: ScanRun[] }>(
      `/discovery/sources/${source.id}/runs?limit=1&cursor=${first.next_cursor}`,
    );
    expect(first.items[0]?.status).toBe('failed');
    expect(second.items[0]?.status).toBe('partial');
  });
  it('validates sources and rejects import/ignore/sync on inappropriate states', async () => {
    const { api, source } = await setup();
    for (const body of [
      { regions: [] },
      { scan_interval_minutes: 4 },
      { role_arn: 'invalid' },
      { external_id: '' },
      { enabled: 'yes' },
    ])
      await expect(
        api(`/discovery/sources/${source.id}`, json('PATCH', body)),
      ).rejects.toMatchObject({ code: 'validation' });
    const resources = await api<ResourcePage>('/discovery/resources');
    const existing = resources.items.find((r) => r.status === 'imported')!;
    const ignored = resources.items.find((r) => r.status === 'ignored')!;
    const gone = resources.items.find((r) => r.status === 'gone')!;
    await expect(
      api(`/discovery/resources/${existing.id}/ignore`, json('POST')),
    ).rejects.toMatchObject({ status: 409 });
    await expect(
      api(`/discovery/resources/${ignored.id}/import`, json('POST', imported)),
    ).rejects.toMatchObject({ status: 409 });
    await expect(
      api(`/discovery/resources/${gone.id}/ignore`, json('POST')),
    ).rejects.toMatchObject({ status: 409 });
    await expect(
      api(`/discovery/resources/${ignored.id}/sync`, json('POST')),
    ).rejects.toMatchObject({ status: 404 });
    await expect(
      api('/discovery/resources?cursor=not-a-cursor'),
    ).rejects.toMatchObject({ code: 'validation' });
  });
  it('preserves ignored inventory and recalculates drift after scans', async () => {
    const { api, source } = await setup();
    const rows = await api<ResourcePage>(
      `/discovery/resources?source_id=${source.id}`,
    );
    const ignored = rows.items.find((r) => r.status === 'ignored')!;
    const importedRow = rows.items.find((r) => r.status === 'imported')!;
    await api(
      `/discovery/resources/${importedRow.id}/sync`,
      json('POST', { password: 'new-password' }),
    );
    await api(`/discovery/sources/${source.id}/scan`, json('POST'));
    const scanned = await api<ResourcePage>(
      `/discovery/resources?source_id=${source.id}`,
    );
    expect(scanned.items.find((r) => r.id === ignored.id)?.status).toBe(
      'ignored',
    );
    expect(scanned.items.find((r) => r.id === importedRow.id)?.drift).toEqual(
      [],
    );
    expect(scanned.counts.new).toBe(rows.counts.new);
  });
  it('diffs only changed fields and distinguishes manual engine changes from syncable endpoints', async () => {
    const { api } = await setup();
    const resource = (
      await api<ResourcePage>('/discovery/resources?status=imported')
    ).items[0]!;
    const cluster = await api<Cluster>(`/clusters/${resource.cluster_id}`);
    expect(
      driftDiff(
        {
          ...cluster,
          engine: resource.engine,
          host: resource.host,
          port: resource.port,
          replica_host: resource.replica_host,
          replica_port: resource.replica_port,
        },
        resource,
      ),
    ).toEqual([]);
    const engine = cluster.engine === 'mysql' ? 'postgres' : 'mysql';
    expect(driftDiff({ ...cluster, engine }, resource)).toContainEqual({
      field: 'engine',
      current: engine,
      discovered: resource.engine,
      applied: false,
    });
    expect(
      driftDiff({ ...cluster, replica_host: null }, resource),
    ).toContainEqual({
      field: 'replica_host',
      current: null,
      discovered: resource.replica_host,
      applied: true,
    });
  });
});

describe('draft tests and faithful discovery inventory', () => {
  it('tests a draft without saving, transfers the result on save, and persists saved tests', async () => {
    const { api } = await setup();
    const draft = {
      provider: 'aws',
      name: 'Test first',
      regions: ['us-east-1'],
      external_id: 'private-secret',
    };
    const before = await api<{ items: DiscoverySource[] }>(
      '/discovery/sources',
    );
    await expect(
      api('/discovery/sources/test', json('POST', draft)),
    ).resolves.toMatchObject({ ok: true });
    expect(
      (await api<{ items: DiscoverySource[] }>('/discovery/sources')).items,
    ).toHaveLength(before.items.length);
    const created = await api<DiscoverySource>(
      '/discovery/sources',
      json('POST', draft),
    );
    expect(created.last_test).toMatchObject({
      ok: true,
      account_id: expect.any(String),
      tested_at: expect.any(String),
    });
    await api(
      `/discovery/sources/${created.id}`,
      json('PATCH', { name: 'Renamed' }),
    );
    expect(
      (
        await api<{ items: DiscoverySource[] }>('/discovery/sources')
      ).items.find((s) => s.id === created.id)?.last_test,
    ).toEqual(created.last_test);
    await expect(
      api(
        `/discovery/sources/${created.id}`,
        json('PATCH', { regions: ['eu-west-1'] }),
      ),
    ).resolves.toMatchObject({ last_test: null });
    await api(`/discovery/sources/${created.id}/test`, json('POST'));
    expect(
      (
        await api<{ items: DiscoverySource[] }>('/discovery/sources')
      ).items.find((s) => s.id === created.id)?.last_test?.ok,
    ).toBe(true);
  });
  it('attaches readers to their primary and leaves stable endpoints alone on rescan', async () => {
    const { api, source } = await setup();
    const before = (await api<ResourcePage>('/discovery/resources')).items;
    expect(
      before.some(
        (r) =>
          r.identifier === 'commerce-reader' ||
          r.identifier === 'sandbox-reader',
      ),
    ).toBe(false);
    expect(
      before.find((r) => r.identifier === 'sandbox-postgres')?.replica_host,
    ).toContain('sandbox-reader');
    await api(`/discovery/sources/${source.id}/scan`, json('POST'));
    const after = (await api<ResourcePage>('/discovery/resources')).items;
    expect(after.map((r) => [r.id, r.host, r.replica_host])).toEqual(
      before.map((r) => [r.id, r.host, r.replica_host]),
    );
    expect(
      after
        .filter((r) => r.host.includes('.new-endpoint.'))
        .map((r) => r.identifier),
    ).toEqual(['commerce-primary']);
  });
});
