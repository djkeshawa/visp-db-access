import type { QueryResult } from '../api/types';
import { cellText, formatTime } from './utils';
type Column = QueryResult['columns'][number];
export function presentCell(value: unknown, column: Column) {
  const raw = value === null ? 'NULL' : cellText(value);
  if (value === null) return { kind: 'null', text: raw, raw };
  if (typeof value === 'object' || /json/i.test(column.type_name)) {
    return {
      kind: 'json',
      text: raw.length > 80 ? `${raw.slice(0, 77)}…` : raw,
      raw,
    };
  }
  if (
    /timestamp|datetime|^date$/i.test(column.type_name) &&
    typeof value === 'string' &&
    Number.isFinite(Date.parse(value))
  ) {
    return { kind: 'timestamp', text: formatTime(value), raw };
  }
  if (
    typeof value === 'number' ||
    /^(?:int\d*|integer|smallint|bigint|numeric|decimal|real|float\d*|double.*)$/i.test(
      column.type_name,
    )
  ) {
    return { kind: 'number', text: raw, raw };
  }
  if (/uuid/i.test(column.type_name)) return { kind: 'uuid', text: raw, raw };
  return { kind: 'text', text: raw, raw };
}
/** Bound work to 40 fetched rows and cap widths so one long value cannot dominate the grid. */
export function sampleWidths(result: QueryResult): number[] {
  return result.columns.map((column, index) => {
    if (/uuid/i.test(column.type_name)) return 172;
    if (/timestamp|datetime|^date$/i.test(column.type_name)) return 216;
    if (/json/i.test(column.type_name)) return 224;
    const length = Math.max(
      column.name.length + 5,
      ...result.rows
        .slice(0, 40)
        .map((row) => presentCell(row[index], column).text.length),
    );
    return Math.ceil(Math.min(320, Math.max(104, length * 7 + 40)) / 4) * 4;
  });
}
