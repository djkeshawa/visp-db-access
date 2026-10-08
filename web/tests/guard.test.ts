import { describe, it, expect } from 'vitest';
import { analyzeDemo } from '../src/api/mock-guard';
import { defaultPolicy } from '../src/api/fixtures';
import { isCidr } from '../src/lib/utils';
describe('demo SQL guard boundaries', () => {
  const policy = defaultPolicy('staging');
  it('does not treat a literal WHERE as a write predicate', () => {
    expect(
      analyzeDemo("UPDATE users SET name = 'WHERE id = 1'", policy).verdict,
    ).toBe('deny');
  });
  it('does not find SQL keywords or LIMITs inside strings', () => {
    expect(
      analyzeDemo("SELECT 'delete LIMIT 1; WHERE' FROM users", policy),
    ).toMatchObject({
      verdict: 'allow',
      rewritten_sql: expect.stringContaining('LIMIT 5001'),
    });
  });
  it('caps an excessive explicit limit', () => {
    expect(
      analyzeDemo('SELECT * FROM users LIMIT 999999', policy).rewritten_sql,
    ).toContain('LIMIT 5001');
  });
  it('blocks protected tables', () => {
    expect(analyzeDemo('SELECT * FROM secrets.tokens', policy).verdict).toBe(
      'deny',
    );
  });
});
describe('CIDR validation', () => {
  it.each([
    '10.0.0.0/8',
    '0.0.0.0/0',
    '127.0.0.1/32',
    '::1/128',
    '2001:db8::/32',
    '2001:db8:1:2:3:4:5:6/64',
  ])('accepts %s', (value) => expect(isCidr(value)).toBe(true));
  it.each([
    '10.0.0.256/8',
    '10.0.0.1/33',
    '10.0.0/8',
    '::1/129',
    '2001:::1/32',
    ':1:2:3:4:5:6:7:8/64',
    '2001::db8::/32',
    '2001:db8::/x',
  ])('rejects %s', (value) => expect(isCidr(value)).toBe(false));
});
