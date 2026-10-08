import type {
  Cluster,
  DiscoveredResource,
  DiscoverySource,
  ScanRun,
} from './types';
import { now, uuid } from './fixtures';
/** Repeatable AWS inventory with ambient and cross-account identities. */
export function discoveryFixtures(clusters: Cluster[]) {
  const time = now();
  const sources: DiscoverySource[] = [
    {
      id: uuid(700),
      provider: 'aws',
      name: 'Production account',
      role_arn: null,
      external_id_set: false,
      regions: ['us-east-1', 'eu-west-1'],
      default_project_id: clusters[0]?.project_id ?? null,
      environment_tag_keys: ['environment', 'env', 'stage'],
      scan_interval_minutes: 60,
      enabled: true,
      last_scan: null,
      last_test: null,
      created_at: time,
      updated_at: time,
    },
    {
      id: uuid(701),
      provider: 'aws',
      name: 'APAC sandbox',
      role_arn: 'arn:aws:iam::222333444555:role/VispDiscovery',
      external_id_set: true,
      regions: ['ap-southeast-2'],
      default_project_id: clusters[1]?.project_id ?? null,
      environment_tag_keys: ['environment', 'env', 'stage'],
      scan_interval_minutes: 120,
      enabled: true,
      last_scan: null,
      last_test: null,
      created_at: time,
      updated_at: time,
    },
  ];
  const names = [
    'commerce-primary',
    'commerce-reader',
    'payments-prod',
    'legacy-mariadb',
    'analytics-staging',
    'orders-aurora',
    'retired-db',
    'auth-dev',
    'sandbox-postgres',
    'sandbox-reader',
    'public-demo',
    'unencrypted-dev',
  ];
  const all: DiscoveredResource[] = names.map((identifier, i) => {
    const source = sources[i < 8 ? 0 : 1]!;
    const region = i < 4 ? 'us-east-1' : i < 8 ? 'eu-west-1' : 'ap-southeast-2';
    const aurora = [0, 2, 5].includes(i);
    const engine = [0, 2, 3].includes(i) ? 'mysql' : 'postgres';
    const environment =
      i >= 7 ? 'development' : i === 4 ? 'staging' : 'production';
    const account = i < 8 ? '111222333444' : '222333444555';
    return {
      id: uuid(800 + i),
      source_id: source.id,
      source_name: source.name,
      provider: 'aws',
      kind: aurora ? 'aurora_cluster' : 'rds_instance',
      arn: `arn:aws:rds:${region}:${account}:${aurora ? 'cluster' : 'db'}:${identifier}`,
      identifier,
      account_id: account,
      region,
      engine,
      engine_detail: aurora
        ? engine === 'postgres'
          ? 'aurora-postgresql'
          : 'aurora-mysql'
        : i === 3
          ? 'mariadb'
          : engine,
      engine_version:
        engine === 'postgres' ? '16.4' : i === 3 ? '10.11.8' : '8.0.39',
      host: `${identifier}${i === 0 ? '.new-endpoint' : ''}.${region}.rds.amazonaws.com`,
      port: engine === 'postgres' ? 5432 : 3306,
      replica_host:
        aurora || i === 4
          ? `${identifier}-reader.${region}.rds.amazonaws.com`
          : null,
      replica_port:
        aurora || i === 4 ? (engine === 'postgres' ? 5432 : 3306) : null,
      database: i === 3 ? null : 'app',
      status_detail: i === 6 ? 'deleted' : 'available',
      publicly_accessible: i === 10,
      encrypted: i !== 11,
      multi_az: aurora,
      iam_auth_enabled: i % 3 === 0,
      vpc_id: 'vpc-demo-private',
      tags: {
        environment,
        ...(i === 1 || i === 9 ? { replica_of: names[i - 1]! } : {}),
      },
      suggested_environment: environment,
      status:
        i === 0
          ? 'imported'
          : i === 6
            ? 'gone'
            : [3, 7].includes(i)
              ? 'ignored'
              : 'new',
      cluster_id: i === 0 ? (clusters[0]?.id ?? null) : null,
      drift: i === 0 ? ['endpoint_changed', 'replica_changed'] : [],
      first_seen_at: time,
      last_seen_at: time,
    };
  });
  for (const replica of all.filter((r) => r.tags.replica_of)) {
    const primary = all.find(
      (r) =>
        r.identifier === replica.tags.replica_of &&
        r.source_id === replica.source_id,
    );
    if (primary) {
      primary.replica_host = replica.host;
      primary.replica_port = replica.port;
    }
  }
  const resources = all.filter((r) => !r.tags.replica_of);
  const runs: ScanRun[] = sources.map((source) => ({
    id: uuid(source.id === uuid(700) ? 900 : 901),
    source_id: source.id,
    status: 'succeeded',
    started_at: time,
    finished_at: time,
    found: resources.filter(
      (r) => r.source_id === source.id && r.status !== 'gone',
    ).length,
    new: resources.filter(
      (r) => r.source_id === source.id && r.status === 'new',
    ).length,
    gone: resources.filter(
      (r) => r.source_id === source.id && r.status === 'gone',
    ).length,
    changed: resources.filter(
      (r) => r.source_id === source.id && r.drift.length,
    ).length,
    errors: [],
  }));
  sources.forEach((source) => {
    source.last_scan = runs.find((run) => run.source_id === source.id) ?? null;
  });
  return { sources, resources, runs };
}
