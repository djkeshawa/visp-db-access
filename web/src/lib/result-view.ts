import type { QueryResult } from '../api/types';
import { cellText, csvCell } from './utils';
import { resultCsv, resultJson } from './result-export';
export type RowSort = { column: number; direction: 'asc' | 'desc' } | null;
/** Filters and sorts only the fetched snapshot; never issues SQL or mutates rows. */
export function fetchedRows(
  rows: unknown[][],
  filter: string,
  sort: RowSort,
): unknown[][] {
  const query = filter.toLocaleLowerCase();
  const filtered = rows.filter(
    (row) =>
      !query ||
      row.some((cell) => cellText(cell).toLocaleLowerCase().includes(query)),
  );
  if (!sort) return filtered;
  const direction = sort.direction === 'asc' ? 1 : -1;
  return filtered.sort((a, b) => {
    const left = a[sort.column],
      right = b[sort.column];
    if (left == null || right == null)
      return direction * (left == null ? (right == null ? 0 : -1) : 1);
    return (
      direction *
      (typeof left === 'number' && typeof right === 'number'
        ? left - right
        : cellText(left).localeCompare(cellText(right), undefined, {
            numeric: true,
          }))
    );
  });
}
export type CopyFormat = 'csv' | 'tsv' | 'markdown' | 'json';
/** Exports the visible fetched snapshot, retaining duplicate names and masked values. */
export function resultText(
  result: Pick<QueryResult, 'columns' | 'rows'>,
  format: CopyFormat,
): string {
  if (format === 'csv') return resultCsv(result);
  if (format === 'json') return resultJson(result);
  const lines: unknown[][] = [
    result.columns.map((c) => c.name),
    ...result.rows,
  ];
  if (format === 'tsv')
    return lines.map((row) => row.map(csvCell).join('\t')).join('\r\n');
  const escape = (value: unknown) =>
    cellText(value)
      .replaceAll('\\', '\\\\')
      .replaceAll('|', '\\|')
      .replace(/\r?\n/g, '<br>');
  const markdown = lines.map((row) => `| ${row.map(escape).join(' | ')} |`);
  markdown.splice(1, 0, `| ${result.columns.map(() => '---').join(' | ')} |`);
  return markdown.join('\n');
}
