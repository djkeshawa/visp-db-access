import { createContext, useContext, type ReactNode } from 'react';
import { Navigate, useLocation } from 'react-router-dom';
import { useResource } from '../../lib/query';
import type { User } from '../../api/types';
import { ErrorPanel, Skeleton } from '../../components/ui';
import { ApiError } from '../../api/errors';
const SessionContext = createContext<User | null>(null);
export const useUser = () => useContext(SessionContext);
export function Session({ children }: { children: ReactNode }) {
  const query = useResource<{ user: User }>('/auth/me');
  const location = useLocation();
  if (query.isPending) return <Skeleton />;
  if (query.error && !query.data) {
    if (query.error instanceof ApiError && query.error.status === 401)
      return (
        <Navigate
          to="/login"
          replace
          state={{ from: location.pathname + location.search }}
        />
      );
    return (
      <ErrorPanel error={query.error} retry={() => void query.refetch()} />
    );
  }
  return (
    <SessionContext.Provider value={query.data?.user ?? null}>
      {children}
    </SessionContext.Provider>
  );
}
export function AdminOnly({ children }: { children: ReactNode }) {
  const user = useUser();
  return user?.org_role === 'admin' ? (
    children
  ) : (
    <ErrorPanel error={new Error('Administrator access is required.')} />
  );
}
