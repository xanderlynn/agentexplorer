import type { Mode, Outcome, Source } from "./api";
import { SOURCES } from "./generated";

export const SOURCE_LABEL = Object.fromEntries(SOURCES.map((s) => [s.id, s.shortLabel])) as Record<Source, string>;

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

export const MODE_LABEL: Record<Mode, string> = {
  coding: "Coding",
  research: "Research",
  running: "Running commands",
  discussion: "Discussion",
};

// Fixed hues so modes read the same in every theme; waiting stays amber.
export const MODE_COLOR: Record<Mode, string> = {
  coding: "#10b981",
  research: "#38bdf8",
  running: "#a78bfa",
  discussion: "#a1a1aa",
};

/** Timeline span kinds from the backend: 0 = waiting on you, 1–4 = modes. */
export const SPAN_MODE: Record<number, Mode | "waiting"> = { 0: "waiting", 1: "coding", 2: "research", 3: "running", 4: "discussion" };

/** Matches trends::DEFAULT_SMART_ZONE_TOKENS in the backend. */
export const SMART_ZONE_DEFAULT = 100_000;

export const STYLE_LABEL: Record<string, string> = {
  one_shot: "One-shot",
  planned: "Planned",
  iterative: "Iterative",
};

/** Filter options for "which tool": all, then each source. */
export const SOURCE_OPTIONS: { value: Source | "all"; label: string }[] = [
  { value: "all", label: "All tools" },
  ...SOURCES.map((s) => ({ value: s.id, label: s.shortLabel })),
];

export const sourceLabel = (s: string) => SOURCE_LABEL[s as Source] ?? s;

/** The app a session ran in, where the tool has more than one. */
const APP_LABEL: Record<string, string> = {
  "claude-desktop": "Claude app",
  "copilot-app": "Copilot app",
};

/** "Claude app" or "Copilot app" for sessions from those apps; else the tool. */
export const appLabel = (source: string, entrypoint?: string | null) => (entrypoint && APP_LABEL[entrypoint]) ?? sourceLabel(source);

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

/**
 * On a subscription (Claude Pro or Max, a Copilot plan) dollars aren't what
 * you pay: amounts are shown as API-equivalent (≈). Set from Settings.
 */
let subscription = false;
export const setSubscription = (on: boolean) => {
  subscription = on;
};
export const onSubscription = () => subscription;

export function money(usd: number | null | undefined, estimate = false): string {
  if (usd == null) return "—";
  const v = usd >= 100 ? `$${Math.round(usd).toLocaleString()}` : `$${usd.toFixed(2)}`;
  return subscription ? `≈${v}` : estimate ? `~${v}` : v;
}

/** A fixed-precision number, or a dash when missing. */
export function fixed(x: number | null | undefined, digits = 1): string {
  return x == null ? "—" : x.toFixed(digits);
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

/** What each kind of agent is for, so a roster reads like a team. */
const ROLES: Record<string, [string, string]> = {
  "general-purpose": ["Generalist", "Multi-step research and changes"],
  explore: ["Scout", "Searches and reads the codebase"],
  plan: ["Architect", "Designs the approach before any code"],
  task: ["Runner", "Runs builds, tests and commands"],
  "code-review": ["Reviewer", "Checks changes for problems"],
  "rubber-duck": ["Sounding board", "Talks a problem through"],
  "claude-code-guide": ["Guide", "Answers questions about the tools"],
  fork: ["Fork", "A copy of the lead, with its context"],
};

/** [title, what it is for] for an agent type from Claude Code or Copilot. */
export function roleOf(role: string): [string, string] {
  const key = role.toLowerCase().replace(/ agent$/, "").trim().replace(/\s+/g, "-");
  if (ROLES[key]) return ROLES[key];
  const words = role.replace(/[-_]/g, " ").replace(/ agent$/i, "");
  return [words.charAt(0).toUpperCase() + words.slice(1), "Custom agent"];
}


/** An agent's own name, or its role's title. */
export const agentName = (a: { name: string | null; role: string }) => a.name ?? roleOf(a.role)[0];

/** Reads the common cron shapes aloud; anything else stays as written. */
export function cronText(cron: string): string {
  const [min, hour, dom, mon, dow] = cron.trim().split(/\s+/);
  const every = min?.match(/^\*\/(\d+)$/);
  if (every && hour === "*" && dom === "*" && mon === "*" && dow === "*") return `every ${every[1]} min`;
  if (/^\d+$/.test(min ?? "") && /^\d+$/.test(hour ?? "") && dom === "*" && mon === "*") {
    const at = `${hour.padStart(2, "0")}:${min.padStart(2, "0")}`;
    if (dow === "*") return `every day at ${at}`;
    if (dow === "1-5") return `weekdays at ${at}`;
  }
  return cron;
}

/** A scheduled job still to come in a live session. */
export function upcoming(j: { cancelled: boolean; recurring: boolean; at: number | null }, live: boolean) {
  return live && !j.cancelled && (j.recurring || (j.at != null && j.at > Date.now()));
}
