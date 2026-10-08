import { describe, it, expect } from 'vitest';
import { resultJson, resultCsv } from '../src/lib/result-export';
describe('result exports', () => {
  it('preserves duplicate column names, NULLs, JSON and masked values', () => {
    const result = {
      columns: [
        { name: 'id', type_name: 'integer', masked: false },
        { name: 'id', type_name: 'integer', masked: false },
        { name: 'email', type_name: 'text', masked: true },
        { name: 'metadata', type_name: 'json', masked: false },
      ],
      rows: [[1, 2, '••••••', { note: null }]],
    };
    expect(JSON.parse(resultJson(result))).toEqual(result);
  });
  it('escapes formula-leading strings in data and column aliases', () => {
    const csv = resultCsv({
      columns: [{ name: '=SUM(1)', type_name: 'text', masked: false }],
      rows: [
        ['  =HYPERLINK("x")'],
        ['@SUM(1)'],
        ['line\n"quoted"'],
        [-5],
        [null],
      ],
    });
    expect(csv).toBe(
      '"\'=SUM(1)"\r\n"\'  =HYPERLINK(""x"")"\r\n"\'@SUM(1)"\r\n"line\n""quoted"""\r\n"-5"\r\n""',
    );
  });
});
