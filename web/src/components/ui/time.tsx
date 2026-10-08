import { useEffect, useState } from 'react';
import { formatTime } from '../../lib/utils';
import { expiryLabel } from '../../lib/sql-diff';
import { relativeTime } from '../../lib/time';
function useClock() {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 60000);
    return () => clearInterval(timer);
  }, []);
  return now;
}
export function RelativeTime({ value }: { value: string | null }) {
  const now = useClock();
  return value ? (
    <time dateTime={value} title={`${formatTime(value)} · ${value}`}>
      {relativeTime(value, now)}
    </time>
  ) : (
    <>—</>
  );
}
export function ExpiryTime({ value }: { value: string }) {
  const now = useClock();
  return (
    <time dateTime={value} title={`${formatTime(value)} · ${value}`}>
      {expiryLabel(value, now)}
    </time>
  );
}
