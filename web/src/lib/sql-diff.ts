/** Linear-space SQL line comparison. Preserves every character of both versions. */
export function sqlDiff(
  before: string,
  after: string,
): { kind: 'same' | 'removed' | 'added'; text: string }[] {
  const left = before.split('\n'),
    right = after.split('\n');
  let prefix = 0,
    suffix = 0;
  while (
    prefix < Math.min(left.length, right.length) &&
    left[prefix] === right[prefix]
  )
    prefix++;
  while (
    suffix < Math.min(left.length, right.length) - prefix &&
    left[left.length - 1 - suffix] === right[right.length - 1 - suffix]
  )
    suffix++;
  return [
    ...left.slice(0, prefix).map((text) => ({ kind: 'same' as const, text })),
    ...left
      .slice(prefix, left.length - suffix)
      .map((text) => ({ kind: 'removed' as const, text })),
    ...right
      .slice(prefix, right.length - suffix)
      .map((text) => ({ kind: 'added' as const, text })),
    ...left
      .slice(left.length - suffix)
      .map((text) => ({ kind: 'same' as const, text })),
  ];
}
export function expiryLabel(expiresAt: string, now: number): string {
  const minutes = Math.ceil((Date.parse(expiresAt) - now) / 60000);
  if (!Number.isFinite(minutes)) return 'Expiry unavailable';
  if (minutes <= 0) return 'Expired';
  return minutes >= 60
    ? `Expires in ${Math.floor(minutes / 60)}h ${minutes % 60}m`
    : `Expires in ${minutes}m`;
}

/** Compare SQL tokens without losing whitespace inside literals, identifiers or comments. */
export function guardedSqlChanged(before: string, after: string): boolean {
  const tokens = (sql: string) => {
    const parts =
      sql.match(
        /(?:E)?'(?:''|\\.|[^'])*'|"(?:""|[^"])*"|`(?:``|[^`])*`|\$([A-Za-z_][A-Za-z_0-9]*|)\$[\s\S]*?\$\1\$|--[^\n]*(?:\n|$)|\/\*[\s\S]*?\*\/|[A-Za-z_][A-Za-z_0-9]*|\d+(?:\.\d+)?|::|<=|>=|<>|!=|\|\||[^\s]/g,
      ) ?? [];
    if (parts.at(-1) === ';') parts.pop();
    return parts;
  };
  return JSON.stringify(tokens(before)) !== JSON.stringify(tokens(after));
}
