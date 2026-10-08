import { Link } from 'react-router-dom';
import { ArrowUpRight } from 'lucide-react';
import type { Cluster, HistoryEntry } from '../../api/types';
import { useResource } from '../../lib/query';
import { ErrorPanel, Skeleton } from '../../components/ui';
import { useUser } from '../auth/session';
import { SetupChecklist } from './setup';
import { MyWork, NeedsYou } from './my-work';
type Overview = {
  clusters_total: number;
  healthy: number;
  degraded: number;
  down: number;
  unknown: number;
  queries_24h: number;
  blocked_24h: number;
  errors_24h: number;
  pending_approvals: number;
  discovery: { new: number; gone: number; drifted: number } | null;
  recent_queries: HistoryEntry[];
  unhealthy_clusters: Cluster[];
};
const count = (n: number, one: string, many = `${one}s`) =>
  `${n.toLocaleString()} ${n === 1 ? one : many}`;
export function OverviewPage() {
  const query = useResource<Overview>('/overview'),
    user = useUser();
  if (query.isPending || query.error)
    return (
      <>
        <div className="page-heading">
          <h1>Workspace overview</h1>
        </div>
        {query.isPending ? (
          <Skeleton />
        ) : (
          <ErrorPanel error={query.error!} retry={() => void query.refetch()} />
        )}
      </>
    );
  const data = query.data;
  return (
    <>
      <div className="page-heading">
        <div>
          <h1 className="overview-greeting" aria-label="Workspace overview">
            Hello, {user?.name.split(' ')[0] ?? 'there'}
          </h1>
          <p className="overview-counts">
            <Link to="/clusters">{count(data.clusters_total, 'cluster')}</Link>{' '}
            ·{' '}
            <Link to="/history">
              {count(data.queries_24h, 'query', 'queries')} in 24 hours
            </Link>{' '}
            · {data.blocked_24h} blocked ·{' '}
            <Link to="/approvals">
              {count(data.pending_approvals, 'pending approval')}
            </Link>
          </p>
        </div>
        <Link to="/console" className="button primary">
          Open console <ArrowUpRight size={16} />
        </Link>
      </div>
      <NeedsYou unhealthy={data.unhealthy_clusters} />
      {user?.org_role === 'admin' && <SetupChecklist />}
      <MyWork />
      {user?.org_role === 'admin' && data.discovery && (
        <p className="overview-counts">
          <Link to="/clusters?tab=discovered">
            Discovery: {data.discovery.new} new · {data.discovery.gone} gone ·{' '}
            {data.discovery.drifted} drifted
          </Link>
        </p>
      )}
    </>
  );
}
