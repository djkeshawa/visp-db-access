import { useInfiniteQuery } from '@tanstack/react-query';
import { params, request } from '../../api/client';
import type { Page } from '../../api/types';
/** Cursor lists retain earlier pages and reset naturally when filters change. */
export function useDiscoveryPages<T, P extends Page<T> = Page<T>>(
  path: string,
  enabled = true,
) {
  return useInfiniteQuery({
    queryKey: ['api', path, 'pages'],
    initialPageParam: null as string | null,
    enabled,
    queryFn: ({ pageParam, signal }) =>
      request<P>(
        `${path}${pageParam ? `${path.includes('?') ? '&' : '?'}${params({ cursor: pageParam }).slice(1)}` : ''}`,
        { signal },
      ),
    getNextPageParam: (last) => last.next_cursor ?? undefined,
    refetchInterval: 15000,
  });
}
