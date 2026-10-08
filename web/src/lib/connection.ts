import type { Cluster } from '../api/types';
/** Fields whose modification requires resupplying the write-only password. */
const endpointFields = [
  'engine',
  'host',
  'port',
  'database',
  'username',
  'tls_mode',
  'replica_host',
  'replica_port',
] as const;
export function endpointChanged(
  current: Cluster,
  patch: Partial<Cluster>,
): boolean {
  return endpointFields.some(
    (field) => field in patch && patch[field] !== current[field],
  );
}
