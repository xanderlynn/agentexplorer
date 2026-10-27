import type { MetricDef } from "./api";
import { money, pct } from "./format";

/** Formats a metric value in its own unit. */
export function fmtMetric(m: Pick<MetricDef, "unit" | "key">, v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return "—";
  if (m.unit === "rate") return pct(v);
  if (m.key === "cost_per_session") return money(v);
  return v.toFixed(1);
}
