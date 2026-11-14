/** Scores how well `query` matches `text` (higher is better; null = no match).
 * Every query character must appear in order; contiguous and word-start
 * matches score higher. */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.trim().toLowerCase();
  if (q === "") return 0;
  const t = text.toLowerCase();
  let score = 0;
  let ti = 0;
  let prev = -2;
  for (const ch of q) {
    const found = t.indexOf(ch, ti);
    if (found === -1) return null;
    score += 1;
    if (found === prev + 1) score += 3;
    if (found === 0 || t[found - 1] === " ") score += 2;
    prev = found;
    ti = found + 1;
  }
  return score - t.length * 0.01;
}
