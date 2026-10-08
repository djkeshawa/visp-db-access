import { useMutation, useQueryClient } from '@tanstack/react-query';
import { request, json } from '../../api/client';
import type { Grant } from '../../api/types';
import { Confirm } from '../../components/ui';
import { useToast } from '../../components/ui/toast';
import { message } from '../../lib/utils';
/** Reports each failed revocation and retains it for an explicit retry. */
export function BulkGrants({
  open,
  onOpenChange,
  grants,
  onRemoved,
}: {
  open: boolean;
  onOpenChange: (value: boolean) => void;
  grants: Grant[];
  onRemoved: (ids: string[]) => void;
}) {
  const cache = useQueryClient(),
    toast = useToast();
  const remove = useMutation({
    mutationFn: async () => {
      const removed: string[] = [],
        errors: string[] = [];
      for (const grant of grants) {
        try {
          await request(`/grants/${grant.id}`, json('DELETE'));
          removed.push(grant.id);
        } catch (error) {
          errors.push(
            `${grant.user.name} on ${grant.scope_name}: ${message(error)}`,
          );
        }
      }
      return { removed, errors };
    },
    onSuccess: (result) => {
      onRemoved(result.removed);
      onOpenChange(false);
      void cache.invalidateQueries({ queryKey: ['api'] });
      if (result.removed.length)
        toast(`${result.removed.length} grants revoked`);
      if (result.errors.length) toast(result.errors.join('; '), 'error');
    },
    retry: false,
  });
  return (
    <Confirm
      open={open}
      onOpenChange={onOpenChange}
      title={`Revoke ${grants.length} selected grants?`}
      description="These users lose the selected grants immediately. Other grants may still provide access. Each revocation is checked separately by the server."
      danger
      busy={remove.isPending}
      onConfirm={() => remove.mutate()}
    >
      <ul className="bulk-review">
        {grants.map((grant) => (
          <li key={grant.id}>
            {grant.user.name} · {grant.level} on {grant.scope_name}
          </li>
        ))}
      </ul>
    </Confirm>
  );
}
