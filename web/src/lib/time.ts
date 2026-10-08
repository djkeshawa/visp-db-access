export function relativeTime(value: string, now: number): string {
  const seconds = Math.floor((now - Date.parse(value)) / 1000);
  if (!Number.isFinite(seconds)) return 'Time unavailable';
  if (seconds < 0) return 'Just now';
  if (seconds < 60) return 'Just now';
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} hr ago`;
  const days = Math.floor(seconds / 86400);
  return `${days} ${days === 1 ? 'day' : 'days'} ago`;
}
