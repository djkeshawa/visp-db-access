import { useState } from 'react';
import * as Dropdown from '@radix-ui/react-dropdown-menu';
import {
  Table2,
  KeyRound,
  RefreshCw,
  TextCursorInput,
  Play,
  X,
} from 'lucide-react';
import type { SchemaTree } from '../../api/types';
import { useResource } from '../../lib/query';
import { Button, ErrorPanel, Skeleton, Empty, Tip } from '../../components/ui';
import { request } from '../../api/client';
import { useQueryClient } from '@tanstack/react-query';
import { useToast } from '../../components/ui/toast';
import { message } from '../../lib/utils';
import { usePopoverLayer } from '../../lib/popover-layer';
export const identifier = (name: string, engine = 'postgres') =>
  engine === 'mysql'
    ? `\`${name.replaceAll('`', '``')}\``
    : `"${name.replaceAll('"', '""')}"`;
export function SchemaExplorer({
  clusterId,
  engine = 'postgres',
  onInsert,
  onQuery,
  onClose,
}: {
  clusterId: string;
  engine?: string;
  onInsert?: (name: string) => void;
  onQuery?: (sql: string) => void;
  onClose?: () => void;
}) {
  const query = useResource<SchemaTree>(`/clusters/${clusterId}/schema`);
  const popoverLayer = usePopoverLayer();
  const [menuTable, setMenuTable] = useState<string | null>(null);
  const [search, setSearch] = useState(''),
    [refreshing, setRefreshing] = useState(false);
  const cache = useQueryClient(),
    toast = useToast();
  const qualified = (schema: string, table: string) =>
    `${identifier(schema, engine)}.${identifier(table, engine)}`;
  return (
    <div className="schema-explorer">
      <div className="section-toolbar">
        <strong>Schema</strong>
        {onClose && (
          <Button
            variant="ghost"
            className="icon"
            aria-label="Close schema panel"
            onClick={onClose}
          >
            <X size={14} />
          </Button>
        )}
        <Button
          variant="ghost"
          aria-label="Refresh schema"
          disabled={refreshing}
          onClick={async () => {
            setRefreshing(true);
            try {
              const data = await request<SchemaTree>(
                `/clusters/${clusterId}/schema?refresh=true`,
              );
              cache.setQueryData(
                ['api', `/clusters/${clusterId}/schema`],
                data,
              );
            } catch (error) {
              toast(message(error), 'error');
            } finally {
              setRefreshing(false);
            }
          }}
        >
          <RefreshCw size={14} />
        </Button>
      </div>
      <input
        aria-label="Search schema"
        placeholder="Find a table or column…"
        value={search}
        onChange={(event) => setSearch(event.target.value)}
      />
      {search &&
        query.data?.schemas.every((namespace) =>
          namespace.tables.every(
            (table) =>
              !`${namespace.name} ${table.name} ${table.columns.map((column) => column.name).join(' ')}`
                .toLowerCase()
                .includes(search.toLowerCase()),
          ),
        ) && (
          <Empty
            title="No matching tables or columns"
            description="Try a shorter name or clear the search."
            action={<Button onClick={() => setSearch('')}>Clear search</Button>}
          />
        )}
      {query.isPending ? (
        <Skeleton />
      ) : query.error ? (
        <ErrorPanel error={query.error} />
      ) : query.data.schemas.length === 0 ? (
        <Empty
          title="No visible tables"
          description="Check your access or refresh the schema."
        />
      ) : (
        query.data.schemas.map((namespace) => (
          <details open key={namespace.name}>
            <summary className="schema-name">{namespace.name}</summary>
            {namespace.tables
              .filter((table) =>
                `${namespace.name} ${table.name} ${table.columns.map((c) => c.name).join(' ')}`
                  .toLowerCase()
                  .includes(search.toLowerCase()),
              )
              .map((table) => (
                <div
                  key={table.name}
                  className="schema-table-group"
                  onContextMenu={(event) => {
                    if (onInsert || onQuery) {
                      event.preventDefault();
                      setMenuTable(qualified(namespace.name, table.name));
                    }
                  }}
                >
                  <details className="schema-table">
                    <summary
                      onKeyDown={(event) => {
                        if (
                          (event.shiftKey && event.key === 'F10') ||
                          event.key === 'ContextMenu'
                        ) {
                          event.preventDefault();
                          setMenuTable(qualified(namespace.name, table.name));
                        }
                      }}
                    >
                      <Table2 size={14} />
                      <span>{table.name}</span>
                      <small>
                        {table.row_estimate?.toLocaleString() ?? 'view'}
                      </small>
                    </summary>
                    {table.columns.map((column) => (
                      <button
                        key={column.name}
                        className="schema-column"
                        title={`${column.data_type}${column.nullable ? ' · nullable' : ' · not null'}`}
                        onClick={() =>
                          onInsert
                            ? onInsert(identifier(column.name, engine))
                            : onQuery?.(
                                `SELECT ${identifier(column.name, engine)} FROM ${qualified(namespace.name, table.name)} LIMIT 100;`,
                              )
                        }
                      >
                        {column.is_primary_key ? (
                          <KeyRound size={12} />
                        ) : (
                          <span className="column-dot" />
                        )}
                        <span>{column.name}</span>
                        <small>
                          {column.data_type}
                          {column.nullable ? ' ?' : ''}
                        </small>
                      </button>
                    ))}
                  </details>
                  <span
                    className="schema-row-actions"
                    onClick={(event) => event.stopPropagation()}
                  >
                    {onInsert && (
                      <Tip text="Insert table name">
                        <Button
                          variant="ghost"
                          className="icon"
                          aria-label={`Insert ${table.name}`}
                          onClick={(event) => {
                            event.preventDefault();
                            onInsert(qualified(namespace.name, table.name));
                          }}
                        >
                          <TextCursorInput size={13} />
                        </Button>
                      </Tip>
                    )}
                    {onQuery && (
                      <Tip text="SELECT top 100">
                        <Button
                          variant="ghost"
                          className="icon"
                          aria-label={`Query ${table.name} top 100`}
                          onClick={(event) => {
                            event.preventDefault();
                            onQuery(
                              `SELECT * FROM ${qualified(namespace.name, table.name)} LIMIT 100;`,
                            );
                          }}
                        >
                          <Play size={13} />
                        </Button>
                      </Tip>
                    )}
                    {(onInsert || onQuery) && (
                      <Dropdown.Root
                        modal={false}
                        open={
                          menuTable === qualified(namespace.name, table.name)
                        }
                        onOpenChange={(open) =>
                          setMenuTable(
                            open ? qualified(namespace.name, table.name) : null,
                          )
                        }
                      >
                        <Dropdown.Trigger asChild>
                          <Button
                            variant="ghost"
                            className="icon schema-menu-trigger"
                            aria-label={`Actions for ${table.name}`}
                          >
                            ⋯
                          </Button>
                        </Dropdown.Trigger>
                        <Dropdown.Portal container={popoverLayer}>
                          <Dropdown.Content className="dropdown">
                            {onInsert && (
                              <Dropdown.Item
                                onSelect={() =>
                                  onInsert(
                                    qualified(namespace.name, table.name),
                                  )
                                }
                              >
                                Insert name
                              </Dropdown.Item>
                            )}
                            {onQuery && (
                              <Dropdown.Item
                                onSelect={() =>
                                  onQuery(
                                    `SELECT * FROM ${qualified(namespace.name, table.name)} LIMIT 100;`,
                                  )
                                }
                              >
                                SELECT top 100
                              </Dropdown.Item>
                            )}
                          </Dropdown.Content>
                        </Dropdown.Portal>
                      </Dropdown.Root>
                    )}
                  </span>
                </div>
              ))}
          </details>
        ))
      )}
    </div>
  );
}
