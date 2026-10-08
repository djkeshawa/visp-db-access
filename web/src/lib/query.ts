import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { request, json } from '../api/client';
import { useToast } from '../components/ui/toast';
import { message } from './utils';
export const useResource = <T>(path: string, enabled = true) =>
  useQuery({
    queryKey: ['api', path],
    queryFn: ({ signal }) => request<T>(path, { signal }),
    enabled,
  });
export function useAction<T = unknown>(success = 'Saved') {
  const cache = useQueryClient(),
    toast = useToast();
  return useMutation({
    mutationFn: ({
      path,
      method = 'POST',
      body,
    }: {
      path: string;
      method?: string;
      body?: unknown;
    }) => request<T>(path, json(method, body)),
    onSuccess: () => {
      void cache.invalidateQueries();
      if (success) toast(success);
    },
    onError: (error) => toast(message(error), 'error'),
  });
}
