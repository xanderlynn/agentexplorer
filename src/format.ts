import type { Outcome, Source } from "./api";

export const SOURCE_LABEL: Record<Source, string> = {
  claude_code: "Claude Code",
  copilot_cli: "Copilot CLI",
  copilot_vscode: "Copilot Chat",
};

export const SOURCE_COLOR: Record<Source, string> = {
  claude_code: "#e07a52",
  copilot_cli: "#5b9df5",
  copilot_vscode: "#a78bfa",
};

export const OUTCOME_LABEL: Record<Outcome, string> = {
  shipped: "Shipped",
  partial: "Partial",
  abandoned: "Abandoned",
  redo: "Redo",
};

export const OUTCOME_CLASS: Record<Outcome, string> = {
  shipped: "bg-emerald-500/15 text-emerald-300 ring-emerald-500/30",
  partial: "bg-amber-500/15 text-amber-300 ring-amber-500/30",
  abandoned: "bg-zinc-500/15 text-zinc-300 ring-zinc-500/30",
  redo: "bg-rose-500/15 text-rose-300 ring-rose-500/30",
};

export const sourceLabel = (s: string) => SOURCE_LABEL[s as Source] ?? s;

export function projectName(path: string | null | undefined): string {
  if (!path) return "—";
  const parts = path.replace(/[\\/]+$/, "").split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export function duration(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) return "—";
  const m = Math.round(ms / 60000);
  if (m < 1) return `${Math.max(0, Math.round(ms / 1000))}s`;
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

export function hours(h: number): string {
  return h < 10 ? `${h.toFixed(1)}h` : `${Math.round(h)}h`;
}

export function money(usd: number | null | undefined, estimate = false): string {
  if (usd == null) return "—";
  const v = usd >= 100 ? `$${Math.round(usd).toLocaleString()}` : `$${usd.toFixed(2)}`;
  return estimate ? `~${v}` : v;
}

export function pct(x: number | null | undefined, digits = 0): string {
  if (x == null || !Number.isFinite(x)) return "—";
  return `${(x * 100).toFixed(digits)}%`;
}

export function compact(n: number): string {
  return Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(n);
}

export function dateTime(ms: number | null | undefined): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

export function date(ms: number | null | undefined): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

export function ago(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h}h ago`;
  return `${Math.round(h / 24)}d ago`;
}

// Relative change from previous to current, for KPI deltas.
export function delta(current: number | null | undefined, previous: number | null | undefined): number | null {
  if (current == null || previous == null || previous === 0) return null;
  return (current - previous) / Math.abs(previous);
}

export const TASK_TYPES = ["feature", "bugfix", "refactor", "tests", "docs", "research", "infra", "review", "other"];
