import type { Policy } from '../api/types';
/** Defaults mirror vda-server Policy::for_environment, including empty protection lists. */
export function environmentPolicy(environment: string): Policy {
  const production = environment === 'production';
  return {
    max_rows: production ? 1000 : 5000,
    statement_timeout_ms: production ? 15000 : 60000,
    lock_timeout_ms: 2000,
    max_cost: null,
    max_concurrent_queries: 4,
    allow_writes: !production,
    require_approval_for_writes: true,
    max_affected_rows: 1000,
    allow_ddl: false,
    route_reads_to_replica: true,
    masked_columns: [],
    blocked_tables: [],
    allowed_cidrs: [],
  };
}
/** Preserve millisecond precision and reject values the server cannot represent. */
export function millisecondsFromSeconds(value: string): number {
  const ms = Number(value) * 1000;
  return value.trim() &&
    Number.isFinite(ms) &&
    Number.isInteger(ms) &&
    ms >= 100 &&
    ms <= 600000
    ? ms
    : NaN;
}
export function policySummary(policy: Policy): string {
  const writes = !policy.allow_writes
    ? 'Writes are blocked.'
    : policy.require_approval_for_writes
      ? "Writes need a second person's approval."
      : 'Writes can run without approval.';
  return `Reads return at most ${policy.max_rows.toLocaleString('en-US')} rows. ${writes} DDL is ${policy.allow_ddl ? 'allowed' : 'blocked'}.`;
}
