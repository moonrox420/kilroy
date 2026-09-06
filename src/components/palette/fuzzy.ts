/**
 * Fuzzy match scorer used by the Command Palette.
 *
 * Three-tier model:
 *   1. Empty query  → match with neutral score; recents bubble up naturally.
 *   2. Exact substring match → very high score, ranked by how early the
 *      match appears in the target.
 *   3. Subsequence match (each query char appears in order in target) →
 *      moderate score, rewarded for consecutive runs.
 *
 * Returns `null` when the query doesn't even match as a subsequence —
 * callers drop those items. `matches` is the list of char indices in
 * the target that were matched, used to highlight them in the UI.
 */

export interface FuzzyResult {
  score: number;
  matches: number[];
}

export function fuzzyScore(query: string, target: string): FuzzyResult | null {
  if (!query) return { score: 1, matches: [] };

  const q = query.toLowerCase();
  const t = target.toLowerCase();

  // Tier 2: exact substring.
  const idx = t.indexOf(q);
  if (idx !== -1) {
    return {
      score: 2000 - idx,
      matches: Array.from({ length: q.length }, (_, i) => idx + i),
    };
  }

  // Tier 3: subsequence.
  const matches: number[] = [];
  let ti = 0;
  for (let qi = 0; qi < q.length; qi++) {
    let found = false;
    while (ti < t.length) {
      if (t[ti] === q[qi]) {
        matches.push(ti);
        ti++;
        found = true;
        break;
      }
      ti++;
    }
    if (!found) return null;
  }

  // Reward tight runs, penalize length, bonus for word-boundary starts.
  let score = 1000;
  for (let i = 1; i < matches.length; i++) {
    if (matches[i] === matches[i - 1] + 1) score += 12;
  }
  if (matches[0] === 0) score += 30;
  else if (
    matches[0] > 0 &&
    [" ", ".", "/", "\\", "_", "-", ":"].includes(t[matches[0] - 1])
  ) {
    score += 15;
  }
  score -= Math.max(0, target.length - q.length) * 0.5;
  return { score, matches };
}

/** Score a target against the query; falls back to the basename for paths. */
export function rankPath(query: string, path: string): FuzzyResult | null {
  // First try matching the whole path; if it fails, try the basename
  // (so `app.tsx` finds `src/App.tsx`).
  const direct = fuzzyScore(query, path);
  if (direct) return direct;
  const slash = path.replace(/\\/g, "/").lastIndexOf("/");
  if (slash === -1) return null;
  const base = path.slice(slash + 1);
  const baseHit = fuzzyScore(query, base);
  if (!baseHit) return null;
  // Shift the highlights into the full-path index space.
  return {
    score: baseHit.score - 50, // slightly less than a direct hit
    matches: baseHit.matches.map((m) => m + slash + 1),
  };
}
