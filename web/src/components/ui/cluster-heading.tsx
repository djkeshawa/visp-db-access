import { ArrowLeft } from 'lucide-react';
import { Link } from 'react-router-dom';
import type { ReactNode } from 'react';
import type { Cluster } from '../../api/types';
import { Badge, HealthBadge, Tip } from './index';

/** A single operational identity line, shared by cluster and query routes. */
export function ClusterHeading({
  cluster,
  children,
}: {
  cluster: Cluster;
  children?: ReactNode;
}) {
  return (
    <div className="cluster-heading-compact">
      <Tip text="All clusters">
        <Link
          className="button ghost icon"
          to="/clusters"
          aria-label="All clusters"
        >
          <ArrowLeft size={16} />
        </Link>
      </Tip>
      <h1>{cluster.name}</h1>
      <span className={`environment-word ${cluster.environment}`}>
        {cluster.environment.charAt(0).toUpperCase() +
          cluster.environment.slice(1)}
      </span>
      <HealthBadge
        status={cluster.health.status}
        latency={cluster.health.latency_ms}
      />
      <span
        className="cluster-endpoint mono muted"
        title={`${cluster.host}:${cluster.port} / ${cluster.database}`}
      >
        {cluster.host}:{cluster.port}
      </span>
      <Badge>{`${cluster.my_access ?? 'No'} access`}</Badge>
      {children}
    </div>
  );
}
