import type { QueryResult } from '../api/types';
import { csvCell } from './utils';
type ResultData = Pick<QueryResult, 'columns' | 'rows'>;
/** Retains column order and duplicate labels instead of dropping values in objects. */
export function resultJson(result: ResultData): string {
  return JSON.stringify(
    { columns: result.columns, rows: result.rows },
    null,
    2,
  );
}
export function resultCsv(result: ResultData): string {
  return [
    result.columns.map((column) => csvCell(column.name)).join(','),
    ...result.rows.map((row) => row.map(csvCell).join(',')),
  ].join('\r\n');
}
