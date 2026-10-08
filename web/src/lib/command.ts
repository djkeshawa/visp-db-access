/** Ordered subsequence search, with a bonus for contiguous and prefix matches. */
export function fuzzyScore(text: string, query: string): number {
  const value = text.toLocaleLowerCase(),
    search = query.trim().toLocaleLowerCase();
  if (!search) return 1;
  if (value.includes(search))
    return 1000 + (value.startsWith(search) ? 100 : 0) - value.length;
  let previous = -1,
    score = 1;
  for (const character of search) {
    const position = value.indexOf(character, previous + 1);
    if (position < 0) return 0;
    score += position === previous + 1 ? 10 : 1;
    previous = position;
  }
  return score;
}
