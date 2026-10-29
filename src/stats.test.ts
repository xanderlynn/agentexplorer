import { describe, expect, it } from "vitest";
import { MIN_SAMPLE } from "./generated";
import { MIN_N, profile, quantile, wilson } from "./stats";

describe("wilson", () => {
  it("has no interval without samples", () => {
    expect(wilson(0, 0)).toBeNull();
  });

  it("brackets the observed rate and stays within 0–1", () => {
    const [lo, hi] = wilson(8, 10)!;
    expect(lo).toBeLessThan(0.8);
    expect(hi).toBeGreaterThan(0.8);
    expect(lo).toBeCloseTo(0.49, 2);
    expect(hi).toBeCloseTo(0.943, 2);
    const [lo0, hi1] = [wilson(0, 5)![0], wilson(5, 5)![1]];
    expect(lo0).toBe(0);
    expect(hi1).toBeLessThanOrEqual(1);
  });

  it("narrows as samples grow", () => {
    const width = ([a, b]: [number, number]) => b - a;
    expect(width(wilson(50, 100)!)).toBeLessThan(width(wilson(5, 10)!));
  });
});

describe("quantile", () => {
  it("interpolates and ignores non-finite values", () => {
    expect(quantile([1, 2, 3, 4], 0.5)).toBe(2.5);
    expect(quantile([4, NaN, 1, Infinity, 3, 2], 0.25)).toBe(1.75);
    expect(quantile([], 0.5)).toBeNull();
  });
});

describe("profile", () => {
  it("needs three points", () => {
    expect(profile([1, null, 2])).toBeNull();
  });

  it("reports the middle half and the best week", () => {
    expect(profile([1, 2, 3, 4, 5])).toEqual({ low: 2, high: 4, best: 5 });
    expect(profile([1, 2, 3, 4, 5], false)?.best).toBe(1);
  });
});

it("uses the engine's minimum sample", () => {
  expect(MIN_N).toBe(MIN_SAMPLE);
});
