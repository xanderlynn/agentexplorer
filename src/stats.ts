// Small statistics helpers for honest displays.

/** Below this many samples a rate is shown as "k/n" and greyed out. */
export const MIN_N = 5;

/** 95% Wilson score interval for a proportion. Works with fractional successes. */
export function wilson(successes: number, n: number): [number, number] | null {
  if (n <= 0) return null;
  const z = 1.96;
  const p = successes / n;
  const denom = 1 + (z * z) / n;
  const centre = (p + (z * z) / (2 * n)) / denom;
  const half = (z * Math.sqrt((p * (1 - p)) / n + (z * z) / (4 * n * n))) / denom;
  return [Math.max(0, centre - half), Math.min(1, centre + half)];
}

export function quantile(values: number[], q: number): number | null {
  const v = values.filter((x) => Number.isFinite(x)).sort((a, b) => a - b);
  if (!v.length) return null;
  const pos = (v.length - 1) * q;
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  return v[lo] + (v[hi] - v[lo]) * (pos - lo);
}

/** The usual range (25th–75th percentile) and best of a weekly series. */
export function profile(series: (number | null)[], higherIsBetter = true) {
  const v = series.filter((x): x is number => x != null && Number.isFinite(x));
  if (v.length < 3) return null;
  return {
    low: quantile(v, 0.25)!,
    high: quantile(v, 0.75)!,
    best: higherIsBetter ? Math.max(...v) : Math.min(...v),
  };
}
