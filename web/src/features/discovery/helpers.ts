import type { Cluster, DiscoveredResource } from '../../api/types';

// Commercial partition only. https://docs.aws.amazon.com/global-infrastructure/latest/regions/aws-regions.html
const regionGroups: Record<string, [string, string][]> = {
  'North America': [
    ['us-east-1', 'N. Virginia'],
    ['us-east-2', 'Ohio'],
    ['us-west-1', 'N. California'],
    ['us-west-2', 'Oregon'],
    ['ca-central-1', 'Canada Central'],
    ['ca-west-1', 'Calgary'],
    ['mx-central-1', 'Mexico Central'],
  ],
  'South America': [['sa-east-1', 'São Paulo']],
  Europe: [
    ['eu-central-1', 'Frankfurt'],
    ['eu-central-2', 'Zurich'],
    ['eu-west-1', 'Ireland'],
    ['eu-west-2', 'London'],
    ['eu-west-3', 'Paris'],
    ['eu-north-1', 'Stockholm'],
    ['eu-south-1', 'Milan'],
    ['eu-south-2', 'Spain'],
  ],
  'Asia Pacific': [
    ['ap-east-1', 'Hong Kong'],
    ['ap-east-2', 'Taipei'],
    ['ap-south-1', 'Mumbai'],
    ['ap-south-2', 'Hyderabad'],
    ['ap-northeast-1', 'Tokyo'],
    ['ap-northeast-2', 'Seoul'],
    ['ap-northeast-3', 'Osaka'],
    ['ap-southeast-1', 'Singapore'],
    ['ap-southeast-2', 'Sydney'],
    ['ap-southeast-3', 'Jakarta'],
    ['ap-southeast-4', 'Melbourne'],
    ['ap-southeast-5', 'Malaysia'],
    ['ap-southeast-6', 'New Zealand'],
    ['ap-southeast-7', 'Thailand'],
  ],
  'Middle East': [
    ['me-south-1', 'Bahrain'],
    ['me-central-1', 'UAE'],
    ['il-central-1', 'Tel Aviv'],
  ],
  Africa: [['af-south-1', 'Cape Town']],
};
/** Searches region code and location without including other AWS partitions. */
export function groupRegions(search = '') {
  const query = search.trim().toLowerCase();
  return Object.entries(regionGroups)
    .map(([geography, values]) => ({
      geography,
      regions: values
        .filter(([code, name]) =>
          `${geography} ${code} ${name}`.toLowerCase().includes(query),
        )
        .map(([code, name]) => ({ code, name })),
    }))
    .filter((group) => group.regions.length);
}
export const driftLabels = {
  endpoint_changed: 'Endpoint changed',
  replica_changed: 'Replica changed',
  engine_changed: 'Engine changed',
  deleted: 'Deleted in AWS',
};
/** Lists real configuration differences, including engine changes that sync cannot apply. */
export function driftDiff(cluster: Cluster, resource: DiscoveredResource) {
  const fields = [
    'engine',
    'host',
    'port',
    'replica_host',
    'replica_port',
  ] as const;
  return fields
    .filter((field) => cluster[field] !== resource[field])
    .map((field) => ({
      field,
      current: cluster[field],
      discovered: resource[field],
      applied: field !== 'engine',
    }));
}
export const minimalPolicy = JSON.stringify(
  {
    Version: '2012-10-17',
    Statement: [
      {
        Effect: 'Allow',
        Action: ['rds:DescribeDBInstances', 'rds:DescribeDBClusters'],
        Resource: '*',
      },
    ],
  },
  null,
  2,
);

/** Mirrors the mock scanner's conservative tag-first environment inference. */
export function suggestEnvironment(
  tags: Record<string, string>,
  identifier: string,
  keys: string[],
): 'production' | 'staging' | 'development' {
  const aliases = {
    production: ['production', 'prod', 'prd', 'live'],
    staging: ['staging', 'stage', 'stg'],
    development: ['development', 'dev', 'test', 'testing', 'sandbox'],
  } as const;
  const match = (value: string) =>
    (Object.keys(aliases) as (keyof typeof aliases)[]).find((environment) =>
      aliases[environment].some(
        (alias) => alias === value.trim().toLowerCase(),
      ),
    );
  for (const key of keys) {
    const environment = match(tags[key] ?? '');
    if (environment) return environment;
  }
  for (const token of identifier.split(/[-_.\s]+/)) {
    const environment = match(token);
    if (environment) return environment;
  }
  return 'production';
}
