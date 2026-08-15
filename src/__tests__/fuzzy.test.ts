/**
 * Fuzzy-search unit tests.
 *
 * Run: npx vitest run src/__tests__/fuzzy.test.ts
 */
import { describe, it, expect } from "vitest";
import { fuzzyScore } from "@/components/palette/fuzzy";

describe("fuzzyScore", () => {
  it("returns 1.0 for exact match", () => {
    expect(fuzzyScore("hello", "hello")?.score).toBe(2000);
  });

  it("returns 0 for no match", () => {
    expect(fuzzyScore("xyz", "hello")).toBeNull();
  });

  it("scores partial matches above 0", () => {
    const exact = fuzzyScore("hello", "hello");
    const partial = fuzzyScore("hl", "hello");
    expect(partial).not.toBeNull();
    expect(partial!.score).toBeGreaterThan(0);
    expect(partial!.score).toBeLessThan(exact!.score);
  });

  it("is case-insensitive", () => {
    expect(fuzzyScore("HELLO", "hello")?.score).toBe(2000);
    expect(fuzzyScore("hello", "HELLO")?.score).toBe(2000);
  });

  it("matches characters in order", () => {
    expect(fuzzyScore("hlo", "hello")).not.toBeNull();
    expect(fuzzyScore("olh", "hello")).toBeNull();
  });
});
