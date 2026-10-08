import { describe, expect, it } from 'vitest';
import {
  environmentPolicy,
  policySummary,
  millisecondsFromSeconds,
} from '../src/lib/policy';
import {
  dayLabel,
  sqlParts,
  statementSummary,
  datePreset,
} from '../src/lib/activity';

describe('policy units and environment defaults', () => {
  it('keeps subsecond limits exact and rejects unbounded values', () => {
    expect(millisecondsFromSeconds('0.125')).toBe(125);
    expect(millisecondsFromSeconds('15')).toBe(15000);
    expect(millisecondsFromSeconds('')).toBeNaN();
    expect(millisecondsFromSeconds('0')).toBeNaN();
    expect(millisecondsFromSeconds('0.0001')).toBeNaN();
    expect(millisecondsFromSeconds('Infinity')).toBeNaN();
  });
  it('matches server defaults and describes actual write restrictions', () => {
    expect(environmentPolicy('production')).toMatchObject({
      max_rows: 1000,
      statement_timeout_ms: 15000,
      allow_writes: false,
      max_cost: null,
      masked_columns: [],
    });
    expect(environmentPolicy('staging')).toMatchObject({
      max_rows: 5000,
      statement_timeout_ms: 60000,
      allow_writes: true,
    });
    expect(policySummary(environmentPolicy('production'))).toBe(
      'Reads return at most 1,000 rows. Writes are blocked. DDL is blocked.',
    );
    expect(policySummary(environmentPolicy('development'))).toContain(
      "Writes need a second person's approval.",
    );
  });
});
describe('activity presentation', () => {
  it('finds verbs and tables without treating literal or comment text as SQL', () => {
    expect(
      statementSummary(
        '-- UPDATE fake\nSELECT \'FROM fake\' FROM public.users JOIN "Order" ON true',
      ),
    ).toBe('SELECT · public.users, "Order"');
    expect(
      statementSummary("UPDATE `users` SET name = 'FROM fake' WHERE id=1"),
    ).toBe('UPDATE · `users`');
    expect(
      sqlParts("SELECT '<script>' -- comment")
        .map((part) => part.text)
        .join(''),
    ).toBe("SELECT '<script>' -- comment");
  });
  it('uses UTC calendar days consistently with loaded-record date filters', () => {
    const now = new Date('2026-10-05T00:30:00Z');
    expect(dayLabel('2026-10-05T00:05:00Z', now)).toBe('Today');
    expect(dayLabel('2026-10-04T23:55:00Z', now)).toBe('Yesterday');
    expect(datePreset(7, now)).toEqual({
      from: '2026-09-29',
      to: '2026-10-05',
    });
  });
});
