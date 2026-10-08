import { useState } from 'react';
import { useInfiniteQuery } from '@tanstack/react-query';
import type { Grant, Page } from '../../api/types';
import { params, request } from '../../api/client';
import { AddGrant, GrantsTable } from './grants';
import { Button, ErrorPanel, Picker, Skeleton } from '../../components/ui';
export function AccessPage() {
  const [adding, setAdding] = useState(false),
    [search, setSearch] = useState(''),
    [scope, setScope] = useState('all'),
    [level, setLevel] = useState('all');
  const filter = { scope: scope === 'all' ? undefined : scope };
  const query = useInfiniteQuery({
    queryKey: ['api', 'grants', filter],
    queryFn: ({ pageParam, signal }) =>
      request<Page<Grant>>(
        `/grants${params({ ...filter, limit: 50, cursor: pageParam })}`,
        { signal },
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
  const items =
    query.data?.pages
      .flatMap((page) => page.items)
      .filter(
        (grant) =>
          `${grant.user.name} ${grant.user.email} ${grant.scope_name}`
            .toLowerCase()
            .includes(search.toLowerCase()) &&
          (scope === 'all' || grant.scope === scope) &&
          (level === 'all' || grant.level === level),
      ) ?? [];
  return (
    <>
      <div className="page-heading">
        <div>
          <h1>Access grants</h1>
          <p>Review user access across projects and clusters.</p>
        </div>
      </div>
      <div className="filterbar">
        <input
          aria-label="Search grants"
          placeholder="Search users or scope names…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <Picker
          value={scope}
          onChange={setScope}
          label="Scope filter"
          options={[
            { value: 'all', label: 'All scopes' },
            { value: 'project', label: 'Projects' },
            { value: 'cluster', label: 'Clusters' },
          ]}
        />
        <Picker
          value={level}
          onChange={setLevel}
          label="Level filter"
          options={[
            { value: 'all', label: 'All levels' },
            ...['read', 'write', 'admin'].map((value) => ({
              value,
              label: value,
            })),
          ]}
        />
      </div>
      {(search || level !== 'all') && (
        <p className="muted">
          Search and level filters apply to loaded grants. Load more to include
          the next page.
        </p>
      )}
      {query.error ? (
        <ErrorPanel error={query.error} />
      ) : query.isPending ? (
        <Skeleton />
      ) : (
        <GrantsTable items={items} onAdd={() => setAdding(true)} />
      )}
      {query.hasNextPage && (
        <Button
          disabled={query.isFetchingNextPage}
          onClick={() => void query.fetchNextPage()}
        >
          {query.isFetchingNextPage ? 'Loading…' : 'Load more grants'}
        </Button>
      )}
      <AddGrant open={adding} onOpenChange={setAdding} />
    </>
  );
}
