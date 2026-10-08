import { describe, expect, it } from 'vitest';
import { guardedSqlChanged } from '../src/lib/sql-diff';
import { relativeTime } from '../src/lib/time';
import { presentCell, sampleWidths } from '../src/lib/result-presentation';
import type { QueryResult } from '../src/api/types';
describe('guarded SQL comparison', () => {
  it('ignores formatting and final semicolons but preserves literal and operator changes', () => {
    expect(
      guardedSqlChanged('SELECT * FROM users;', 'SELECT  *\nFROM users'),
    ).toBe(false);
    expect(
      guardedSqlChanged('SELECT * FROM users', 'SELECT * FROM users LIMIT 100'),
    ).toBe(true);
    expect(guardedSqlChanged("SELECT 'a b'", "SELECT 'ab'")).toBe(true);
    expect(guardedSqlChanged('SELECT "a b"', 'SELECT "ab"')).toBe(true);
    expect(
      guardedSqlChanged('SELECT $tag$a b$tag$', 'SELECT $tag$ab$tag$'),
    ).toBe(true);
    expect(guardedSqlChanged('SELECT a >= 2', 'SELECT a > = 2')).toBe(true);
  });
});
describe('result presentation', () => {
  it('bounds sampled widths and treats identifiers and structured data deliberately', () => {
    const result = {
      columns: [
        { name: 'id', type_name: 'uuid' },
        { name: 'body', type_name: 'text' },
        { name: 'n', type_name: 'int8' },
      ],
      rows: [['abc', 'x'.repeat(10000), 1234]],
    } as QueryResult;
    expect(sampleWidths(result)).toEqual([172, 320, 104]);
    expect(presentCell(1234, result.columns[2]!).kind).toBe('number');
    expect(
      presentCell(
        { plan: 'trial' },
        { name: 'profile', type_name: 'jsonb', masked: false },
      ),
    ).toMatchObject({ kind: 'json', raw: '{"plan":"trial"}' });
    expect(
      presentCell('2026-10-02T10:20:00Z', {
        name: 'created',
        type_name: 'timestamp',
        masked: false,
      }),
    ).toMatchObject({ kind: 'timestamp', raw: '2026-10-02T10:20:00Z' });
  });
});
it('formats ages independently of timezone and handles malformed dates', () => {
  const now = Date.parse('2026-10-02T12:00:00Z');
  expect(relativeTime('2026-10-02T11:48:00Z', now)).toBe('12 min ago');
  expect(relativeTime('2026-10-01T12:00:00Z', now)).toBe('1 day ago');
  expect(relativeTime('bad', now)).toBe('Time unavailable');
});
