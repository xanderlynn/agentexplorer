import { useQuery } from "@tanstack/react-query";
import { Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api, type PayoffGroup, type ProgressT } from "../api";
import { Card, Empty, ErrorNote, Loading, PageHeader } from "../components/ui";
import { date, duration, money, pct } from "../format";
import { chart } from "../theme";

/** Rated sessions a group needs before its success rate is shown as more than noise. */
const SOLID = 20;

/**
 * Did I get better, and what works? A year month by month (success, cost per
 * shipped, reply time, waiting) with one headline, and results by sub-agent
 * team size, app and kind of work, with how many sessions each rests on.
 */
export default function Progress() {
  const q = useQuery({ queryKey: ["progress", 365], queryFn: () => api.progress(365) });
  const header = <PageHeader title="Progress" subtitle="Your last year, month by month, and what pays off. Built from session summaries, so it covers your whole history." />;
  if (q.isLoading) return <>{header}<Loading /></>;
  if (q.error) return <>{header}<ErrorNote error={q.error} /></>;
  const p = q.data!;
  if (p.months.length < 2) return <>{header}<Empty title="Not enough history yet">Progress shows once you have two months of sessions.</Empty></>;
  return (
    <>
      {header}
      {p.headline && <p className="mb-4 rounded-xl border border-zinc-800 bg-zinc-900/60 px-4 py-3 text-sm text-zinc-200">{p.headline}</p>}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <MonthChart p={p} title="Success" value={(m) => (m.success != null && m.rated >= 5 ? m.success * 100 : null)} fmt={(v) => `${Math.round(v)}%`} />
        <MonthChart p={p} title="Cost per shipped session" value={(m) => m.costPerShipped} fmt={(v) => money(v)} />
        <MonthChart p={p} title="Typical reply" value={(m) => (m.replyMs != null ? m.replyMs / 60_000 : null)} fmt={(v) => duration(v * 60_000)} />
        <MonthChart p={p} title="Hours agents waited on you" value={(m) => m.waitedMs / 3_600_000} fmt={(v) => `${Math.round(v)}h`} />
      </div>
      <Finished />
      <h2 className="mb-2 mt-6 text-sm font-medium text-zinc-200">What pays off</h2>
      <p className="mb-3 text-xs text-zinc-500">
        Success over rated sessions (shipped 1, partial ½). Groups with fewer than {SOLID} rated sessions are greyed: too few to trust.
      </p>
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
        <Payoff title="By sub-agent team size" rows={p.byTeam} />
        <Payoff title="By app" rows={p.byApp} />
        <Payoff title="By kind of work" rows={p.byKind} />
      </div>
    </>
  );
}

function MonthChart({ p, title, value, fmt }: { p: ProgressT; title: string; value: (m: ProgressT["months"][number]) => number | null; fmt: (v: number) => string }) {
  const data = p.months.map((m) => ({ month: m.month.slice(2), v: value(m) }));
  return (
    <Card title={title}>
      <div className="h-40" role="img" aria-label={`${title} by month`}>
        <ResponsiveContainer>
          <LineChart data={data} margin={{ left: 0, right: 8, top: 4, bottom: 0 }}>
            <XAxis dataKey="month" {...chart.axisProps} />
            <YAxis {...chart.axisProps} width={48} tickFormatter={(v) => fmt(v as number)} />
            <Tooltip {...chart.tooltip} formatter={(v) => fmt(v as number)} />
            <Line type="monotone" dataKey="v" name={title} stroke={chart.accent} strokeWidth={2} dot={false} connectNulls isAnimationActive={false} />
          </LineChart>
        </ResponsiveContainer>
      </div>
    </Card>
  );
}

function Payoff({ title, rows }: { title: string; rows: PayoffGroup[] }) {
  return (
    <Card title={title}>
      <table className="w-full text-sm">
        <thead className="text-xs text-zinc-500">
          <tr>
            <th className="pb-1 text-left font-medium" />
            <th className="pb-1 text-right font-medium">Success</th>
            <th className="pb-1 text-right font-medium">Avg cost</th>
            <th className="pb-1 text-right font-medium">Rated</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((g) => (
            <tr key={g.label} className={g.rated < SOLID ? "text-zinc-500" : "text-zinc-200"}>
              <td className="py-1 pr-2">{g.label}</td>
              <td className="py-1 text-right tabular-nums">{g.success != null ? pct(g.success) : "—"}</td>
              <td className="py-1 text-right tabular-nums">{money(g.avgCostUsd)}</td>
              <td className="py-1 text-right tabular-nums text-zinc-500">{g.rated}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Card>
  );
}

/** Workstreams you finished over the year, by quarter: what you shipped. */
function Finished() {
  const q = useQuery({ queryKey: ["finished", 365], queryFn: () => api.finishedWorkstreams(365) });
  const list = q.data ?? [];
  if (!list.length) return null;
  const quarter = (t: number) => {
    const d = new Date(t);
    return `${d.getFullYear()} Q${Math.floor(d.getMonth() / 3) + 1}`;
  };
  const byQuarter = new Map<string, typeof list>();
  for (const f of list) byQuarter.set(quarter(f.finishedAt), [...(byQuarter.get(quarter(f.finishedAt)) ?? []), f]);
  return (
    <Card title="What you finished" className="mt-4">
      <div className="space-y-4">
        {[...byQuarter].map(([qtr, items]) => (
          <details key={qtr} open={qtr === quarter(Date.now())}>
            <summary className="cursor-pointer text-sm text-zinc-200">
              {qtr} <span className="text-zinc-500">· {items.length} workstreams · {money(items.reduce((a, f) => a + f.costUsd, 0))}</span>
            </summary>
            <ul className="mt-1 divide-y divide-zinc-800/60">
              {items.slice(0, 50).map((f) => (
                <li key={f.key} className="flex items-center gap-3 py-1.5 text-sm">
                  <span className="min-w-0 flex-1 truncate text-zinc-300">{f.title}</span>
                  <span className="shrink-0 text-xs text-zinc-500">
                    {f.tasks} tasks · {f.sessions} sessions · {duration(Math.max(0, f.finishedAt - f.startedAt))} · {date(f.finishedAt)}
                  </span>
                </li>
              ))}
              {items.length > 50 && <li className="py-1.5 text-xs text-zinc-500">and {items.length - 50} more</li>}
            </ul>
          </details>
        ))}
      </div>
    </Card>
  );
}
