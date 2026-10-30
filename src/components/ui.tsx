import type { ButtonHTMLAttributes, ReactNode } from "react";
import { ArrowDownRight, ArrowUpRight, Loader2 } from "lucide-react";
import type { GroupStat, Outcome, Source } from "../api";
import { OUTCOMES } from "../api";
import { OUTCOME_CLASS, OUTCOME_LABEL, SOURCE_COLOR, hours, pct, appLabel } from "../format";
import { MIN_N, wilson } from "../stats";

export function cx(...parts: (string | false | null | undefined)[]) {
  return parts.filter(Boolean).join(" ");
}

export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 className="text-xl font-semibold tracking-tight text-zinc-50">{title}</h1>
        {subtitle && <p className="mt-1 text-sm text-zinc-400">{subtitle}</p>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

export function Card({ children, className, title, action }: { children: ReactNode; className?: string; title?: ReactNode; action?: ReactNode }) {
  return (
    <section className={cx("rounded-xl border border-zinc-800/80 bg-zinc-900/60 p-4", className)}>
      {(title || action) && (
        <div className="mb-3 flex items-center justify-between gap-2">
          {title && <h2 className="text-sm font-medium text-zinc-300">{title}</h2>}
          {action}
        </div>
      )}
      {children}
    </section>
  );
}

export function Stat({
  label,
  value,
  hint,
  change,
  goodWhenUp = true,
  points = false,
  spark,
  usual,
  n,
  onClick,
  quiet,
}: {
  label: string;
  value: ReactNode;
  hint?: ReactNode;
  change?: number | null;
  goodWhenUp?: boolean;
  /** Show the change as percentage points rather than a relative change. */
  points?: boolean;
  /** Weekly values, oldest first, drawn as a sparkline. */
  spark?: (number | null)[];
  /** Your usual range and best, e.g. "usually 40–60%, best 71%". */
  usual?: ReactNode;
  /** Sample size behind the value. */
  n?: ReactNode;
  onClick?: () => void;
  /** Said instead, small, when the value and every week of it are zero: a tile that never moves is noise. */
  quiet?: string;
}) {
  const dead = quiet != null && (value === 0 || value === "0" || value === "0.0") && (spark ?? []).every((x) => !x);
  if (dead) {
    return (
      <div className="rounded-xl border border-dashed border-zinc-800/80 px-4 py-3 text-left">
        <span className="text-xs font-medium uppercase tracking-wide text-zinc-600">{label}</span>
        <p className="mt-1 text-xs text-zinc-500">{quiet}</p>
      </div>
    );
  }
  const showChange = change != null && Number.isFinite(change) && Math.abs(change) >= 0.005;
  const good = showChange && change! > 0 === goodWhenUp;
  const Tag = onClick ? "button" : "div";
  return (
    <Tag
      onClick={onClick}
      className={cx(
        "rounded-xl border border-zinc-800/80 bg-zinc-900/60 px-4 py-3 text-left",
        onClick && "transition-colors hover:border-zinc-700 hover:bg-zinc-800/40",
      )}
    >
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-xs font-medium uppercase tracking-wide text-zinc-500">{label}</span>
        {n != null && <span className="text-xs tabular-nums text-zinc-500">n={n}</span>}
      </div>
      <div className="mt-1 flex items-baseline gap-2">
        <span className="text-2xl font-semibold tabular-nums text-zinc-50">{value}</span>
        {showChange && (
          <span className={cx("inline-flex items-center text-xs tabular-nums", good ? "text-emerald-400" : "text-rose-400")}>
            {change! > 0 ? <ArrowUpRight size={12} aria-hidden /> : <ArrowDownRight size={12} aria-hidden />}
            <span className="sr-only">{change! > 0 ? "up" : "down"}</span>
            {points ? `${Math.round(Math.abs(change!) * 100)} pts` : pct(Math.abs(change!))}
          </span>
        )}
        {spark && <Sparkline values={spark} className="ml-auto" />}
      </div>
      {usual && <div className="mt-0.5 text-xs text-zinc-400">{usual}</div>}
      {hint && <div className="mt-0.5 text-xs text-zinc-500">{hint}</div>}
    </Tag>
  );
}

/** `fromZero` draws counts against zero rather than stretching their range. */
export function Sparkline({ values, className, fromZero }: { values: (number | null)[]; className?: string; fromZero?: boolean }) {
  const pts = values.map((v, i) => [i, v] as const).filter((p): p is readonly [number, number] => p[1] != null);
  if (pts.length < 2) return null;
  const w = 72;
  const h = 22;
  const max = Math.max(...pts.map((p) => p[1]));
  const min = fromZero ? 0 : Math.min(...pts.map((p) => p[1]));
  const span = max - min || 1;
  const x = (i: number) => (i / (values.length - 1)) * w;
  const y = (v: number) => h - 2 - ((v - min) / span) * (h - 4);
  const d = pts.map(([i, v], k) => `${k ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(" ");
  const last = pts[pts.length - 1];
  return (
    <svg width={w} height={h} className={className} aria-hidden>
      <path d={d} fill="none" stroke="var(--color-emerald-500)" strokeWidth={1.5} strokeLinejoin="round" />
      <circle cx={x(last[0])} cy={y(last[1])} r={2} fill="var(--color-emerald-500)" />
    </svg>
  );
}

/**
 * A success rate that is honest about its sample: "k/n" in grey below
 * MIN_N, otherwise a percentage with a 95% interval in the tooltip and a
 * whisker on the bar.
 */
export function Rate({ rate, n, bar = true }: { rate: number | null; n: number; bar?: boolean }) {
  if (rate == null || n === 0) return <span className="text-xs text-zinc-500">—</span>;
  const ci = wilson(rate * n, n);
  if (n < MIN_N) {
    return (
      <span className="text-xs tabular-nums text-zinc-500" title={`Only ${n} rated; too few to compare`}>
        {Number.isInteger(rate * n) ? rate * n : (rate * n).toFixed(1)}/{n} rated
      </span>
    );
  }
  const title = ci ? `${pct(rate)} (95% range ${pct(ci[0])}–${pct(ci[1])}, n=${n})` : undefined;
  return (
    <span className="flex items-center gap-2" title={title}>
      {bar && (
        <span className="relative h-1.5 w-full min-w-12 overflow-hidden rounded-full bg-zinc-800">
          {ci && <span className="absolute h-full bg-zinc-600" style={{ left: `${ci[0] * 100}%`, width: `${(ci[1] - ci[0]) * 100}%` }} />}
          <span className="absolute h-full rounded-full bg-emerald-500" style={{ width: `${rate * 100}%`, opacity: 0.9 }} />
        </span>
      )}
      <span className="w-14 shrink-0 text-right text-xs tabular-nums text-zinc-300">
        {pct(rate)} <span className="text-zinc-500">({n})</span>
      </span>
    </span>
  );
}

/** Marks a number derived from heuristics rather than measured directly. */
export function Heuristic({ title }: { title: string }) {
  return (
    <span className="ml-1 cursor-help rounded border border-zinc-700 px-1 text-[0.7rem] font-normal normal-case tracking-normal text-zinc-400" title={title}>
      ≈ heuristic
    </span>
  );
}

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md";
  busy?: boolean;
};

export function Button({ variant = "secondary", size = "md", busy, className, children, disabled, ...rest }: ButtonProps) {
  return (
    <button
      {...rest}
      disabled={disabled || busy}
      className={cx(
        "inline-flex items-center justify-center gap-1.5 rounded-lg font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50",
        size === "sm" ? "px-2.5 py-1 text-xs" : "px-3.5 py-1.5 text-sm",
        variant === "primary" && "bg-emerald-500 text-[var(--color-on-accent)] hover:bg-emerald-400",
        variant === "secondary" && "border border-zinc-700 bg-zinc-800/70 text-zinc-200 hover:bg-zinc-700/70",
        variant === "ghost" && "text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100",
        variant === "danger" && "border border-rose-900 bg-rose-950/40 text-rose-300 hover:bg-rose-900/40",
        className,
      )}
    >
      {busy && <Loader2 size={14} className="animate-spin" />}
      {children}
    </button>
  );
}

export const inputBase =
  "rounded-lg border border-zinc-700 bg-zinc-950/60 px-3 py-1.5 text-sm text-zinc-100 placeholder:text-zinc-500 focus:border-emerald-500/70 focus:outline-none";
export const inputClass = `${inputBase} w-full`;

export function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: ReactNode }) {
  return (
    <label className="block">
      <span className="mb-1 block text-xs font-medium text-zinc-400">{label}</span>
      {children}
      {hint && <span className="mt-1 block text-xs text-zinc-500">{hint}</span>}
    </label>
  );
}

/** The tool, or the app when it has one (Claude app, Copilot app). */
export function SourceBadge({ source, entrypoint }: { source: Source | string; entrypoint?: string | null }) {
  const color = SOURCE_COLOR[source as Source] ?? "#a1a1aa";
  return (
    <span className="inline-flex items-center gap-1.5 whitespace-nowrap text-xs text-zinc-300">
      <span className="h-2 w-2 rounded-full" style={{ background: color }} />
      {appLabel(source, entrypoint)}
    </span>
  );
}

export function OutcomeBadge({ outcome }: { outcome: Outcome | null }) {
  if (!outcome) return <span className="text-xs text-zinc-500">Unrated</span>;
  return (
    <span className={cx("inline-flex rounded-md px-1.5 py-0.5 text-xs font-medium ring-1 ring-inset", OUTCOME_CLASS[outcome])}>
      {OUTCOME_LABEL[outcome]}
    </span>
  );
}

export function OutcomePicker({ value, onChange }: { value: Outcome | null; onChange: (o: Outcome | null) => void }) {
  return (
    <div className="inline-flex flex-wrap gap-1.5">
      {OUTCOMES.map((o) => (
        <button
          key={o}
          type="button"
          onClick={() => onChange(value === o ? null : o)}
          className={cx(
            "rounded-md px-2.5 py-1 text-xs font-medium ring-1 ring-inset transition-colors",
            value === o ? OUTCOME_CLASS[o] : "text-zinc-400 ring-zinc-700 hover:bg-zinc-800",
          )}
        >
          {OUTCOME_LABEL[o]}
        </button>
      ))}
    </div>
  );
}

export function Rating({ value, onChange }: { value: number | null; onChange: (n: number | null) => void }) {
  return (
    <div className="inline-flex gap-1">
      {[1, 2, 3, 4, 5].map((n) => (
        <button
          key={n}
          type="button"
          aria-label={`Rate ${n} of 5`}
          onClick={() => onChange(value === n ? null : n)}
          className={cx(
            "h-7 w-7 rounded-md text-xs font-semibold ring-1 ring-inset transition-colors",
            value != null && n <= value ? "bg-emerald-500/20 text-emerald-300 ring-emerald-500/40" : "text-zinc-500 ring-zinc-700 hover:bg-zinc-800",
          )}
        >
          {n}
        </button>
      ))}
    </div>
  );
}

export function Segmented<T extends string | number>({
  options,
  value,
  onChange,
}: {
  options: { value: T; label: string }[];
  value: T;
  onChange: (v: T) => void;
}) {
  return (
    <div className="inline-flex rounded-lg border border-zinc-800 bg-zinc-900 p-0.5">
      {options.map((o) => (
        <button
          key={String(o.value)}
          type="button"
          onClick={() => onChange(o.value)}
          className={cx(
            "rounded-md px-2.5 py-1 text-xs font-medium transition-colors",
            o.value === value ? "bg-zinc-700 text-zinc-50" : "text-zinc-400 hover:text-zinc-200",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export const RANGE_OPTIONS = [
  { value: 7, label: "7d" },
  { value: 30, label: "30d" },
  { value: 90, label: "90d" },
  { value: 0, label: "All" },
];

export function Empty({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-zinc-800 px-6 py-12 text-center">
      <div className="text-sm font-medium text-zinc-300">{title}</div>
      {children && <div className="mt-1 max-w-md text-sm text-zinc-500">{children}</div>}
    </div>
  );
}

export function Loading() {
  return (
    <div className="flex items-center gap-2 py-10 text-sm text-zinc-500">
      <Loader2 size={16} className="animate-spin" /> Loading…
    </div>
  );
}

export function ErrorNote({ error }: { error: unknown }) {
  if (!error) return null;
  return (
    <div className="rounded-lg border border-rose-900/60 bg-rose-950/30 px-3 py-2 text-sm text-rose-300">{String(error)}</div>
  );
}

// Horizontal bar used in tables, 0..1.
export function Meter({ value, color = "var(--color-emerald-500)" }: { value: number | null; color?: string }) {
  const w = value == null ? 0 : Math.max(0, Math.min(1, value)) * 100;
  return (
    <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
      <div className="h-full rounded-full" style={{ width: `${w}%`, background: color }} />
    </div>
  );
}

export function Legend({ items }: { items: [string, string][] }) {
  return (
    <span className="flex gap-3 text-xs text-zinc-400">
      {items.map(([label, color]) => (
        <span key={label} className="inline-flex items-center gap-1.5">
          <span className="h-2.5 w-2.5 rounded-sm" style={{ background: color }} />
          {label}
        </span>
      ))}
    </span>
  );
}

export function GroupTable({ rows, label, onRow }: { rows: GroupStat[]; label: (key: string) => ReactNode; onRow?: (key: string) => void }) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="text-left text-xs text-zinc-500">
            <th className="pb-2 font-medium" />
            <th className="pb-2 text-right font-medium">Sessions</th>
            <th className="pb-2 text-right font-medium">Active</th>
            <th className="pb-2 text-right font-medium">Commits</th>
            <th className="w-40 pb-2 pl-4 font-medium">Success (rated)</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-800/60">
          {rows.map((g) => (
            <tr key={g.key} className={onRow ? "cursor-pointer hover:bg-zinc-800/30" : undefined} onClick={onRow ? () => onRow(g.key) : undefined}>
              <td className="max-w-[14rem] py-1.5 pr-2">{label(g.key)}</td>
              <td className="py-1.5 text-right tabular-nums text-zinc-300">{g.sessions}</td>
              <td className="py-1.5 text-right tabular-nums text-zinc-300">{hours(g.activeHours)}</td>
              <td className="py-1.5 text-right tabular-nums text-zinc-300">{g.commits}</td>
              <td className="py-1.5 pl-4">
                <Rate rate={g.successRate} n={g.rated} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** A small labelled figure in a bordered tile. */
export function Metric({ label, value, hint }: { label: string; value: ReactNode; hint?: string }) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 px-3 py-2">
      <div className="text-xs uppercase tracking-wide text-zinc-500">{label}</div>
      <div className="mt-0.5 text-sm font-semibold tabular-nums text-zinc-100">{value}</div>
      {hint && <div className="text-xs text-zinc-500">{hint}</div>}
    </div>
  );
}
