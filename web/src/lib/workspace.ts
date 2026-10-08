export type QueryTab = {
  id: string;
  name: string;
  sql: string;
  savedSql: string;
};
/** Accepts the existing draft format and rejects malformed browser storage. */
export function loadDrafts(text: string | null): QueryTab[] {
  try {
    const stored: unknown = JSON.parse(text ?? 'null');
    if (!Array.isArray(stored)) return [];
    return stored
      .filter(
        (tab): tab is QueryTab =>
          !!tab &&
          typeof tab === 'object' &&
          typeof tab.id === 'string' &&
          typeof tab.sql === 'string' &&
          typeof tab.name === 'string',
      )
      .map((tab) => ({
        ...tab,
        savedSql: typeof tab.savedSql === 'string' ? tab.savedSql : tab.sql,
      }));
  } catch {
    return [];
  }
}
export function moveTab(
  tabs: QueryTab[],
  from: number,
  to: number,
): QueryTab[] {
  if (from < 0 || to < 0 || from >= tabs.length || to >= tabs.length)
    return tabs;
  const result = [...tabs],
    tab = result.splice(from, 1)[0];
  if (tab) result.splice(to, 0, tab);
  return result;
}
export function closeTab(tabs: QueryTab[], index: number, active: string) {
  const remaining = tabs.filter((_, i) => i !== index);
  return {
    tabs: remaining,
    active: remaining.some((tab) => tab.id === active)
      ? active
      : (remaining[Math.max(0, index - 1)]?.id ?? ''),
  };
}
/** Policy review is completed by an actual saved policy update from the audit API. */
export function setupSteps(state: {
  clusters: number;
  policiesReviewed: number;
  users: number;
  queries: number;
}): boolean[] {
  return [
    state.clusters > 0,
    state.policiesReviewed > 0,
    state.users > 1,
    state.queries > 0,
  ];
}
