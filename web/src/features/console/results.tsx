import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import * as Dropdown from '@radix-ui/react-dropdown-menu';
import { Copy, Download, LockKeyhole, MoreHorizontal } from 'lucide-react';
import type { QueryResult } from '../../api/types';
import { useMedia } from '../../lib/media';
import { Button, Badge, Modal, Tip } from '../../components/ui';
import { presentCell, sampleWidths } from '../../lib/result-presentation';
import { cellText, download, message } from '../../lib/utils';
import { useToast } from '../../components/ui/toast';
import { usePopoverLayer } from '../../lib/popover-layer';
import {
  fetchedRows,
  resultText,
  type CopyFormat,
  type RowSort,
} from '../../lib/result-view';
/** Virtualized, keyboard-accessible interaction with the fetched result snapshot. */
export function Results({ result }: { result: QueryResult }) {
  const popoverLayer = usePopoverLayer();
  const scroll = useRef<HTMLDivElement>(null),
    toast = useToast();
  const [filter, setFilter] = useState(''),
    [sort, setSort] = useState<RowSort>(null),
    [hidden, setHidden] = useState<number[]>([]),
    [pinned, setPinned] = useState<number[]>([]),
    [widths, setWidths] = useState<Record<number, number>>({}),
    [expanded, setExpanded] = useState<{
      value: unknown;
      name: string;
      row: number;
    } | null>(null),
    [focus, setFocus] = useState({ row: 0, column: 0 });
  const rows = useMemo(
    () => fetchedRows(result.rows, filter, sort),
    [result.rows, filter, sort],
  );
  const visible = result.columns
    .map((column, index) => ({ column, index }))
    .filter(({ index }) => !hidden.includes(index))
    .sort(
      (a, b) =>
        Number(pinned.includes(b.index)) - Number(pinned.includes(a.index)),
    );
  const sampled = useMemo(() => sampleWidths(result), [result]);
  const width = (index: number) => widths[index] ?? sampled[index] ?? 104;
  const template = `46px ${visible.map(({ index }) => `${width(index)}px`).join(' ')}`;
  const totalWidth = visible.reduce((sum, { index }) => sum + width(index), 46);
  const pinnedLeft = (index: number) =>
    46 +
    visible
      .slice(
        0,
        visible.findIndex((c) => c.index === index),
      )
      .reduce((sum, c) => sum + width(c.index), 0);
  const rowHeight = useMedia('(pointer: coarse), (max-width: 600px)') ? 44 : 40;
  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroll.current,
    estimateSize: () => rowHeight,
    overscan: 12,
  });
  useEffect(() => {
    virtual.measure();
  }, [rowHeight, virtual]);
  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast('Copied to clipboard');
    } catch (error) {
      toast(message(error), 'error');
    }
  };
  const snapshot = {
    columns: visible.map((c) => c.column),
    rows: rows.map((row) => visible.map((c) => row[c.index])),
  };
  const exportResult = (format: 'csv' | 'json') => {
    download(
      `query-result.${format}`,
      resultText(snapshot, format),
      format === 'json' ? 'application/json' : 'text/csv;charset=utf-8',
    );
    toast('Exported fetched rows');
  };
  const inspect = (row: number, column: number) => {
    const col = visible[column];
    if (col)
      setExpanded({
        value: rows[row]?.[col.index],
        name: col.column.name,
        row: row + 1,
      });
  };
  const move = (row: number, column: number) => {
    const next = {
      row: Math.max(0, Math.min(rows.length - 1, row)),
      column: Math.max(0, Math.min(visible.length - 1, column)),
    };
    setFocus(next);
    virtual.scrollToIndex(next.row, { align: 'auto' });
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        scroll.current
          ?.querySelector<HTMLElement>(
            `[data-cell="${next.row}-${next.column}"]`,
          )
          ?.focus(),
      ),
    );
  };
  return (
    <section className="results" aria-label="Result snapshot">
      <div className="section-toolbar">
        <div className="inline wrap">
          <strong>Results</strong>
          <Badge variant="status">Completed</Badge>
        </div>
        <div className="inline wrap">
          <Dropdown.Root modal={false}>
            <Dropdown.Trigger asChild>
              <Button>
                <Download size={14} />
                Export
              </Button>
            </Dropdown.Trigger>
            <Dropdown.Portal container={popoverLayer}>
              <Dropdown.Content className="dropdown">
                <Dropdown.Item onSelect={() => exportResult('csv')}>
                  Download CSV
                </Dropdown.Item>
                <Dropdown.Item onSelect={() => exportResult('json')}>
                  Download JSON
                </Dropdown.Item>
                <Dropdown.Separator />
                {(['csv', 'tsv', 'markdown', 'json'] as CopyFormat[]).map(
                  (format) => (
                    <Dropdown.Item
                      key={format}
                      onSelect={() => void copy(resultText(snapshot, format))}
                    >
                      Copy as{' '}
                      {format === 'markdown'
                        ? 'Markdown'
                        : format.toUpperCase()}
                    </Dropdown.Item>
                  ),
                )}
              </Dropdown.Content>
            </Dropdown.Portal>
          </Dropdown.Root>
        </div>
      </div>
      {result.affected_rows !== null && (
        <div className="callout success">
          {result.affected_rows.toLocaleString()} row
          {result.affected_rows === 1 ? '' : 's'} affected.
        </div>
      )}
      {result.columns.length > 0 && (
        <>
          <div className="results-controls">
            <input
              aria-label="Filter fetched rows"
              placeholder="Filter fetched rows…"
              value={filter}
              onChange={(event) => {
                setFilter(event.target.value);
                setFocus({ row: 0, column: 0 });
              }}
            />
            <small className="muted">
              Search and sort apply to fetched rows only.
            </small>
            {hidden.length > 0 && (
              <Button onClick={() => setHidden([])}>
                Show all columns ({hidden.length} hidden)
              </Button>
            )}
            {sort && <Button onClick={() => setSort(null)}>Clear sort</Button>}
          </div>
          <div
            ref={scroll}
            className="result-scroll"
            role="grid"
            aria-label="Query results"
            aria-rowcount={rows.length + 1}
            aria-colcount={visible.length + 1}
            tabIndex={rows.length && visible.length ? undefined : 0}
          >
            <div
              className="result-header"
              role="row"
              aria-rowindex={1}
              style={{ gridTemplateColumns: template, width: totalWidth }}
            >
              <div
                role="columnheader"
                className="pinned-column"
                style={{ left: 0 }}
              >
                #
              </div>
              {visible.map(({ column, index }) => (
                <div
                  role="columnheader"
                  aria-label={column.name}
                  aria-colindex={
                    visible.findIndex((c) => c.index === index) + 2
                  }
                  aria-sort={
                    sort?.column === index
                      ? sort.direction === 'asc'
                        ? 'ascending'
                        : 'descending'
                      : 'none'
                  }
                  key={index}
                  className={`result-column ${pinned.includes(index) ? 'pinned-column' : ''}`}
                  style={
                    pinned.includes(index)
                      ? { left: pinnedLeft(index) }
                      : undefined
                  }
                >
                  <span>
                    {column.masked && <LockKeyhole size={12} />} {column.name}
                    {sort?.column === index && (
                      <span aria-hidden="true">
                        {sort.direction === 'asc' ? '↑' : '↓'}
                      </span>
                    )}
                    <Dropdown.Root modal={false}>
                      <Dropdown.Trigger asChild>
                        <button
                          className="column-menu"
                          aria-label={`Column options for ${column.name}`}
                        >
                          <MoreHorizontal size={14} />
                        </button>
                      </Dropdown.Trigger>
                      <Dropdown.Portal container={popoverLayer}>
                        <Dropdown.Content className="dropdown">
                          <Dropdown.Item
                            onSelect={() =>
                              setSort({ column: index, direction: 'asc' })
                            }
                          >
                            Sort ascending
                          </Dropdown.Item>
                          <Dropdown.Item
                            onSelect={() =>
                              setSort({ column: index, direction: 'desc' })
                            }
                          >
                            Sort descending
                          </Dropdown.Item>
                          <Dropdown.Item
                            onSelect={() =>
                              void copy(
                                resultText(
                                  {
                                    columns: [column],
                                    rows: rows.map((row) => [row[index]]),
                                  },
                                  'tsv',
                                ),
                              )
                            }
                          >
                            Copy column
                          </Dropdown.Item>
                          <Dropdown.Item
                            onSelect={() =>
                              setHidden((values) => [...values, index])
                            }
                          >
                            Hide column
                          </Dropdown.Item>
                          <Dropdown.Item
                            onSelect={() =>
                              setPinned((values) =>
                                values.includes(index)
                                  ? values.filter((i) => i !== index)
                                  : [...values, index],
                              )
                            }
                          >
                            {pinned.includes(index) ? 'Unpin' : 'Pin'} column
                          </Dropdown.Item>
                        </Dropdown.Content>
                      </Dropdown.Portal>
                    </Dropdown.Root>
                  </span>
                  <small>
                    {column.type_name}
                    {column.masked ? ' · masked' : ''}
                  </small>
                  <div
                    className="resize-handle"
                    role="separator"
                    aria-label={`Resize ${column.name}`}
                    aria-orientation="vertical"
                    aria-valuenow={width(index)}
                    aria-valuemin={90}
                    aria-valuemax={1000}
                    tabIndex={0}
                    onKeyDown={(event) => {
                      if (
                        event.key === 'ArrowRight' ||
                        event.key === 'ArrowLeft'
                      ) {
                        event.preventDefault();
                        setWidths((values) => ({
                          ...values,
                          [index]: Math.max(
                            90,
                            Math.min(
                              1000,
                              width(index) +
                                (event.key === 'ArrowRight' ? 20 : -20),
                            ),
                          ),
                        }));
                      }
                    }}
                    onPointerDown={(event) => {
                      const element = event.currentTarget;
                      element.setPointerCapture(event.pointerId);
                      const start = event.clientX,
                        initial = width(index);
                      const move = (event: PointerEvent) =>
                        setWidths((values) => ({
                          ...values,
                          [index]: Math.min(
                            1000,
                            Math.max(90, initial + event.clientX - start),
                          ),
                        }));
                      const end = () => {
                        element.removeEventListener('pointermove', move);
                        element.removeEventListener('pointerup', end);
                        element.removeEventListener('lostpointercapture', end);
                      };
                      element.addEventListener('pointermove', move);
                      element.addEventListener('pointerup', end);
                      element.addEventListener('lostpointercapture', end);
                    }}
                  />
                </div>
              ))}
            </div>
            <div
              role="rowgroup"
              style={{
                height: virtual.getTotalSize(),
                width: totalWidth,
                position: 'relative',
              }}
            >
              {virtual.getVirtualItems().map((item) => (
                <div
                  className="result-row"
                  role="row"
                  aria-rowindex={item.index + 2}
                  key={item.key}
                  style={{
                    position: 'absolute',
                    top: item.start,
                    height: item.size,
                    gridTemplateColumns: template,
                    width: totalWidth,
                  }}
                >
                  <Tip text="Copy row as TSV">
                    <button
                      role="gridcell"
                      className="row-number pinned-column"
                      style={{ left: 0 }}
                      aria-colindex={1}
                      aria-label={`Copy row ${item.index + 1}`}
                      tabIndex={-1}
                      onClick={() =>
                        void copy(
                          resultText(
                            {
                              columns: snapshot.columns,
                              rows: [snapshot.rows[item.index] ?? []],
                            },
                            'tsv',
                          ),
                        )
                      }
                    >
                      {item.index + 1}
                    </button>
                  </Tip>
                  {visible.map(({ column, index }, position) => {
                    const value = rows[item.index]?.[index];
                    const cell = presentCell(value, column);
                    return (
                      <div
                        role="gridcell"
                        key={index}
                        aria-colindex={position + 2}
                        className={`result-cell cell-${cell.kind} ${value === null ? 'null' : ''} ${pinned.includes(index) ? 'pinned-column' : ''}`}
                        style={
                          pinned.includes(index)
                            ? { left: pinnedLeft(index) }
                            : undefined
                        }
                      >
                        <button
                          className="cell-value"
                          data-cell={`${item.index}-${position}`}
                          tabIndex={
                            focus.row === item.index &&
                            Math.min(focus.column, visible.length - 1) ===
                              position
                              ? 0
                              : -1
                          }
                          title={cell.raw}
                          onFocus={() =>
                            setFocus({ row: item.index, column: position })
                          }
                          onClick={() => inspect(item.index, position)}
                          onKeyDown={(event) => {
                            const offsets: Record<string, [number, number]> = {
                              ArrowRight: [0, 1],
                              ArrowLeft: [0, -1],
                              ArrowDown: [1, 0],
                              ArrowUp: [-1, 0],
                            };
                            const offset = offsets[event.key];
                            if (offset) {
                              event.preventDefault();
                              move(
                                item.index + offset[0],
                                position + offset[1],
                              );
                            } else if (
                              event.key === 'Home' ||
                              event.key === 'End'
                            ) {
                              event.preventDefault();
                              move(
                                event.ctrlKey
                                  ? event.key === 'Home'
                                    ? 0
                                    : rows.length - 1
                                  : item.index,
                                event.key === 'Home' ? 0 : visible.length - 1,
                              );
                            }
                          }}
                        >
                          <span
                            className={
                              cell.kind === 'json' ? 'json-chip' : undefined
                            }
                          >
                            {cell.text}
                          </span>
                        </button>
                        <button
                          className="cell-copy"
                          tabIndex={-1}
                          aria-label={`Copy ${column.name} row ${item.index + 1}`}
                          onClick={() => void copy(cellText(value))}
                        >
                          <Copy size={12} />
                        </button>
                      </div>
                    );
                  })}
                </div>
              ))}
            </div>
          </div>
          {rows.length === 0 && (
            <div className="result-empty">
              {result.rows.length === 0
                ? 'No rows returned. Check the WHERE condition or try a different table.'
                : 'No fetched rows match this filter.'}
              {filter && (
                <Button onClick={() => setFilter('')}>Clear filter</Button>
              )}
            </div>
          )}
        </>
      )}
      <div className="result-status" role="status">
        <span>
          {result.row_count.toLocaleString()} rows fetched
          {filter ? ` · ${rows.length} matching` : ''} · {result.elapsed_ms} ms
        </span>
        <span>
          Routed to <strong>{result.routed_to}</strong>
        </span>
        {result.columns.some((column) => column.masked) && (
          <span>
            <LockKeyhole size={12} />{' '}
            <span className="status-long">
              Protected columns stay masked in copies and exports.
            </span>
            <span className="status-short">Masked columns stay masked</span>
          </span>
        )}
        {result.truncated && (
          <span className="warning-text">
            <span className="status-long">
              Showing first {result.row_count} rows — limit set by policy.
              Narrow the query to see other rows.
            </span>
            <span className="status-short">
              First {result.row_count.toLocaleString()} rows (policy limit)
            </span>
          </span>
        )}
      </div>
      <Modal
        open={!!expanded}
        onOpenChange={(open) => {
          if (!open) setExpanded(null);
        }}
        drawer
        title={
          expanded?.value !== null && typeof expanded?.value === 'object'
            ? 'JSON value'
            : 'Cell value'
        }
        description={
          expanded
            ? `${expanded.name} · fetched row ${expanded.row}`
            : undefined
        }
      >
        <pre className="json-preview" tabIndex={0}>
          {expanded?.value === null
            ? 'NULL'
            : typeof expanded?.value === 'object'
              ? JSON.stringify(expanded.value, null, 2)
              : cellText(expanded?.value)}
        </pre>
        <Button
          onClick={() =>
            void copy(
              typeof expanded?.value === 'object'
                ? JSON.stringify(expanded?.value, null, 2)
                : cellText(expanded?.value),
            )
          }
        >
          <Copy size={14} />
          {expanded?.value !== null && typeof expanded?.value === 'object'
            ? 'Copy JSON'
            : 'Copy value'}
        </Button>
      </Modal>
    </section>
  );
}
