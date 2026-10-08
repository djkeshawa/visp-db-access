import { useEffect, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { request, json } from '../api/client';
import type { User } from '../api/types';
import { useUser } from '../features/auth/session';
import { Button, Field, Modal } from './ui';
import { loginMessage } from '../lib/utils';
/** Reauthenticates the same identity without unloading the current route or editor. */
export function SessionRecovery() {
  const user = useUser(),
    cache = useQueryClient();
  const [expired, setExpired] = useState(false),
    [offline, setOffline] = useState(false),
    [busy, setBusy] = useState(false),
    [error, setError] = useState('');
  useEffect(() => {
    const expire = () => setExpired(true),
      unreachable = () => setOffline(true);
    window.addEventListener('vda:session-expired', expire);
    window.addEventListener('vda:unreachable', unreachable);
    return () => {
      window.removeEventListener('vda:session-expired', expire);
      window.removeEventListener('vda:unreachable', unreachable);
    };
  }, []);
  useEffect(() => {
    if (!offline) return;
    const retry = async () => {
      try {
        await request('/auth/me');
        setOffline(false);
        void cache.invalidateQueries({ queryKey: ['api'] });
      } catch {
        /* Reads retry; mutations always require a new user action. */
      }
    };
    const timer = setInterval(() => void retry(), 10000);
    const online = () => void retry();
    window.addEventListener('online', online);
    return () => {
      clearInterval(timer);
      window.removeEventListener('online', online);
    };
  }, [offline, cache]);
  return (
    <>
      {offline && (
        <div className="connection-banner" role="status">
          Cannot reach the gateway. Reconnecting every 10 seconds.
          <Button
            onClick={() =>
              void request('/auth/me')
                .then(() => {
                  setOffline(false);
                  void cache.invalidateQueries({ queryKey: ['api'] });
                })
                .catch(() => undefined)
            }
          >
            Retry now
          </Button>
        </div>
      )}
      <Modal
        open={expired}
        onOpenChange={setExpired}
        title="Your session expired"
        description="Sign in again to continue. Your page and query drafts are still here."
      >
        <form
          onSubmit={async (event) => {
            event.preventDefault();
            setBusy(true);
            setError('');
            try {
              const data = new FormData(event.currentTarget);
              const value = await request<{ user: User }>(
                '/auth/login',
                json('POST', {
                  email: user?.email,
                  password: data.get('password'),
                }),
              );
              if (value.user.id !== user?.id) {
                await request('/auth/logout', json('POST'));
                throw new Error(
                  'Sign in with the same account to restore this workspace.',
                );
              }
              cache.setQueryData(['api', '/auth/me'], value);
              setExpired(false);
              void cache.invalidateQueries();
            } catch (error) {
              setError(loginMessage(error));
            } finally {
              setBusy(false);
            }
          }}
        >
          <Field label="Email">
            <input value={user?.email ?? ''} readOnly autoComplete="username" />
          </Field>
          <Field label="Password">
            <input
              name="password"
              type="password"
              required
              autoComplete="current-password"
            />
          </Field>
          {error && (
            <p className="error-text" role="alert">
              {error}
            </p>
          )}
          <div className="dialog-actions">
            <Button type="submit" variant="primary" disabled={busy}>
              {busy ? 'Signing in…' : 'Resume session'}
            </Button>
          </div>
        </form>
      </Modal>
    </>
  );
}
