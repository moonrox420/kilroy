/**
 * Fuzzy-search unit tests.
 *
 * Run: npx vitest run src/__tests__/fuzzy.test.ts
 */
import { describe, it, expect } from "vitest";

// Simple fuzzy match scorer used by the command palette.
function fuzzyScore(query: string, target: string): number {
  const q = query.toLowerCase();
  const t = target.toLowerCase();
  let qi = 0;
  let score = 0;
  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      score += 1;
      qi++;
    }
  }
  return qi === q.length ? score / q.length : 0;
}

describe("fuzzyScore", () => {
  it("returns 1.0 for exact match", () => {
    expect(fuzzyScore("hello", "hello")).toBe(1.0);
  });

  it("returns 0 for no match", () => {
    expect(fuzzyScore("xyz", "hello")).toBe(0);
  });

  it("scores partial matches above 0", () => {
    const s = fuzzyScore("hl", "hello");
    expect(s).toBeGreaterThan(0);
    expect(s).toBeLessThan(1);
  });

  it("is case-insensitive", () => {
    expect(fuzzyScore("HELLO", "hello")).toBe(1.0);
    expect(fuzzyScore("hello", "HELLO")).toBe(1.0);
  });

  it("matches characters in order", () => {
    expect(fuzzyScore("hlo", "hello")).toBeGreaterThan(0);
    expect(fuzzyScore("olh", "hello")).toBe(0);
  });
});