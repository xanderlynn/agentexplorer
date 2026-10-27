import { useQuery } from "@tanstack/react-query";
import { Bar, BarChart, CartesianGrid, Legend, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api, type Benchmark, type Patterns as PatternsT } from "../api";
import { Bar as Meter, Card, Empty, ErrorNote, Loading, OutcomeBadge, PageHeader, RANGE_OPTIONS, Segmented, SourceBadge } from "../components/ui";
import { duration, pct } from "../format";
import { chart, heat } from "../theme";

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

export default function Patterns({ days, setDays, onOpen }: { days: number; setDays: (d: number) => void; onOpen: (id: string) => void }) {
  const q = useQuery({ queryKey: ["patterns", days], queryFn: () => api.patterns(days) });
  const header = (
    <PageHeader
      title="Patterns"
      subtitle="When you work, what you delegate, how the agent works, and whether you're getting better at it."
      actions={<Segmented options={RANGE_OPTIONS} value={days} onChange={setDays} />}
    />
  );
  if (q.isLoading) return <>{header}<Loading /></>;
  if (q.error) return <>{header}<ErrorNote error={q.error} /></>;
  if (!q.data) return header;
  const p = q.data;

  return (
    <>
      {header}
      <Card title="Activity over the last year">
        <Calendar days={p.calendar} />
      </Card>

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="When you start sessions">
          <HourHeatmap grid={p.heatmap} />
        </Card>
        <Card title="Friction (heuristic, 5 = least)">
          {p.friction ? (
            <div className="space-y-3">
              <FrictionBar label="Requirements" hint="clear goal in the opening prompt" value={p.friction.requirements} />
              <FrictionBar label="Steering" hint="few corrections and interruptions" value={p.friction.steering} />
              <FrictionBar label="Scope" hint="session kept to a bounded size" value={p.friction.scope} />
              <div className="border-t border-zinc-800 pt-3 text-sm text-zinc-300">
                Overall <span className="font-semibold text-zinc-50">{p.friction.overall.toFixed(2)}</span> / 5
              </div>
            </div>
          ) : (
            <p className="text-sm text-zinc-500">No sessions in range.</p>
          )}
        </Card>
      </div>

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="What you delegate">
          {p.categories.length === 0 ? (
            <p className="text-sm text-zinc-500">No sessions in range.</p>
          ) : (
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-xs text-zinc-500">
                  <th className="pb-2 font-medium">Category</th>
                  <th className="pb-2 text-right font-medium">Sessions</th>
                  <th className="pb-2 text-right font-medium">Avg active</th>
                  <th className="pb-2 text-right font-medium">Friction</th>
                  <th className="w-32 pb-2 pl-4 font-medium">Success</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-zinc-800/60">
                {p.categories.map((c) => (
                  <tr key={c.category}>
                    <td className="py-1.5 capitalize text-zinc-200">{c.category}</td>
                    <td className="py-1.5 text-right tabular-nums text-zinc-300">{c.sessions}</td>
                    <td className="py-1.5 text-right tabular-nums text-zinc-300">{Math.round(c.avgActiveMinutes)}m</td>
                    <td className="py-1.5 text-right tabular-nums text-zinc-300">{c.avgFriction.toFixed(1)}</td>
                    <td className="py-1.5 pl-4">
                      <div className="flex items-center gap-2">
                        <Meter value={c.successRate} />
                        <span className="w-9 text-right text-xs tabular-nums text-zinc-400">{c.rated ? pct(c.successRate) : "—"}</span>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <p className="mt-2 text-xs text-zinc-500">Category is guessed from the opening prompt.</p>
        </Card>
        <Card title="What the agents do">
          <ToolMix p={p} />
          <div className="mt-3 h-52">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={p.tools} layout="vertical" margin={{ left: 10, right: 16 }}>
                <XAxis type="number" hide />
                <YAxis type="category" dataKey="tool" width={130} tick={{ fill: chart.axis, fontSize: 11 }} />
                <Tooltip {...chart.tooltip} cursor={{ fill: chart.grid }} />
                <Bar dataKey="count" name="Calls" fill={chart.accent} radius={[0, 3, 3, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </Card>
      </div>

      <Card title="Learning curve" className="mt-4">
        {p.qualityWeekly.length < 2 ? (
          <p className="text-sm text-zinc-500">Needs at least two weeks of sessions.</p>
        ) : (
          <div className="h-64">
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={p.qualityWeekly} margin={{ left: -10, right: 8, top: 8 }}>
                <CartesianGrid stroke={chart.grid} vertical={false} />
                <XAxis dataKey="weekStart" tick={{ fill: chart.axis, fontSize: 11 }} tickFormatter={(d: string) => d.slice(5)} />
                <YAxis yAxisId="rate" tick={{ fill: chart.axis, fontSize: 11 }} tickFormatter={(v: number) => `${Math.round(v * 100)}%`} domain={[0, 1]} />
                <YAxis yAxisId="score" orientation="right" tick={{ fill: chart.axis, fontSize: 11 }} domain={[0, 5]} />
                <Tooltip
                  {...chart.tooltip}
                  formatter={(v, name) => (String(name).includes("rate") || String(name).includes("Success") ? pct(Number(v)) : Number(v).toFixed(2))}
                />
                <Legend wrapperStyle={{ fontSize: 12 }} />
                <Line yAxisId="rate" type="monotone" dataKey="successRate" name="Success" stroke={chart.accent} strokeWidth={2} dot={false} connectNulls />
                <Line yAxisId="rate" type="monotone" dataKey="reEditRate" name="Re-edit rate" stroke="var(--color-amber-500)" strokeWidth={2} dot={false} connectNulls />
                <Line yAxisId="rate" type="monotone" dataKey="editsWithoutReadRate" name="Blind-edit rate" stroke="var(--color-rose-500)" strokeWidth={2} dot={false} connectNulls />
                <Line yAxisId="score" type="monotone" dataKey="avgFriction" name="Friction score" stroke="var(--color-sky-500)" strokeWidth={2} dot={false} />
                <Line yAxisId="score" type="monotone" dataKey="correctionsPerSession" name="Corrections / session" stroke={chart.axis} strokeDasharray="4 3" dot={false} />
              </LineChart>
            </ResponsiveContainer>
          </div>
        )}
        <p className="mt-2 text-xs text-zinc-500">
          Re-edit rate: edits to a file already changed in the same session. Blind-edit rate: edits to files the agent never read. Lower is better for both.
        </p>
      </Card>

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="Then vs now">
          {p.thenVsNow ? (
            <BenchTable rows={p.thenVsNow} />
          ) : (
            <Empty title="Not enough history yet">Shows your first 30 days against the last 30 once you have two months of data.</Empty>
          )}
        </Card>
        <Card title="Context and delegation">
          <BenchTable rows={[p.withoutCompaction, p.withCompaction, p.withSubagents]} />
        </Card>
      </div>

      <Card title="Longest sessions" className="mt-4">
        <ul className="divide-y divide-zinc-800/70">
          {p.longest.map((s) => (
            <li key={s.id}>
              <button className="flex w-full items-center gap-3 py-2 text-left hover:bg-zinc-800/30" onClick={() => onOpen(s.id)}>
                <span className="w-16 shrink-0 text-right text-sm tabular-nums text-zinc-200">{duration(s.activeMs)}</span>
                <span className="min-w-0 flex-1 truncate text-sm text-zinc-300">{s.title ?? "Untitled"}</span>
                <SourceBadge source={s.source} />
                <span className="w-20 text-right text-xs text-zinc-500">{s.userTurns} prompts</span>
                <OutcomeBadge outcome={s.outcome} />
              </button>
            </li>
          ))}
        </ul>
      </Card>
    </>
  );
}

function Calendar({ days }: { days: PatternsT["calendar"] }) {
  if (days.length === 0) return null;
  const max = Math.max(1, ...days.map((d) => d.activeMinutes));
  // Pad the start so columns are Monday-first weeks.
  const first = new Date(days[0].date + "T00:00:00");
  const pad = (first.getDay() + 6) % 7;
  const cells: (PatternsT["calendar"][number] | null)[] = [...Array(pad).fill(null), ...days];
  const weeks: (typeof cells)[] = [];
  for (let i = 0; i < cells.length; i += 7) weeks.push(cells.slice(i, i + 7));
  const total = days.reduce((n, d) => n + d.sessions, 0);
  const activeDays = days.filter((d) => d.sessions > 0).length;
  return (
    <div>
      <div className="flex gap-[3px] overflow-x-auto pb-1">
        {weeks.map((w, i) => (
          <div key={i} className="flex flex-col gap-[3px]">
            {w.map((d, j) =>
              d ? (
                <div
                  key={j}
                  className="h-[11px] w-[11px] rounded-[2px]"
                  style={{ background: heat(d.activeMinutes / max) }}
                  title={`${d.date}: ${d.sessions} sessions, ${Math.round(d.activeMinutes)} active min`}
                />
              ) : (
                <div key={j} className="h-[11px] w-[11px]" />
              ),
            )}
          </div>
        ))}
      </div>
      <div className="mt-2 text-xs text-zinc-500">
        {total} sessions on {activeDays} days in the last year · colour = active minutes
      </div>
    </div>
  );
}

function HourHeatmap({ grid }: { grid: number[][] }) {
  const max = Math.max(1, ...grid.flat());
  return (
    <div className="overflow-x-auto">
      <div className="inline-grid gap-[3px]" style={{ gridTemplateColumns: `2.5rem repeat(24, minmax(14px, 1fr))` }}>
        <div />
        {Array.from({ length: 24 }, (_, h) => (
          <div key={h} className="text-center text-[10px] text-zinc-500">{h % 3 === 0 ? h : ""}</div>
        ))}
        {grid.map((row, d) => (
          <div key={d} className="contents">
            <div className="text-[11px] leading-[14px] text-zinc-500">{WEEKDAYS[d]}</div>
            {row.map((n, h) => (
              <div key={h} className="h-[14px] rounded-[2px]" style={{ background: heat(n / max) }} title={`${WEEKDAYS[d]} ${h}:00 — ${n} sessions`} />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function FrictionBar({ label, hint, value }: { label: string; hint: string; value: number }) {
  return (
    <div>
      <div className="flex items-baseline justify-between text-sm">
        <span className="text-zinc-200">{label}</span>
        <span className="tabular-nums text-zinc-300">{value.toFixed(1)}</span>
      </div>
      <Meter value={value / 5} />
      <div className="mt-0.5 text-xs text-zinc-500">{hint}</div>
    </div>
  );
}

function ToolMix({ p }: { p: PatternsT }) {
  const total = p.reads + p.edits + p.shellCalls + p.otherToolCalls;
  const parts = [
    { label: "Read / search", n: p.reads, color: "var(--color-sky-500)" },
    { label: "Edit / write", n: p.edits, color: chart.accent },
    { label: "Shell", n: p.shellCalls, color: "var(--color-amber-500)" },
    { label: "Other", n: p.otherToolCalls, color: chart.muted },
  ];
  if (total === 0) return <p className="text-sm text-zinc-500">No tool calls in range.</p>;
  return (
    <div>
      <div className="flex h-2.5 overflow-hidden rounded-full">
        {parts.map((x) => (
          <div key={x.label} style={{ width: `${(x.n / total) * 100}%`, background: x.color }} title={`${x.label}: ${x.n}`} />
        ))}
      </div>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-zinc-400">
        {parts.map((x) => (
          <span key={x.label} className="inline-flex items-center gap-1.5">
            <span className="h-2 w-2 rounded-full" style={{ background: x.color }} />
            {x.label} {pct(x.n / total)}
          </span>
        ))}
        <span>· reads per edit {p.edits ? (p.reads / p.edits).toFixed(1) : "—"}</span>
      </div>
    </div>
  );
}

function BenchTable({ rows }: { rows: Benchmark[] }) {
  const metric = (label: string, f: (b: Benchmark) => string) => (
    <tr key={label}>
      <td className="py-1.5 text-zinc-400">{label}</td>
      {rows.map((b) => (
        <td key={b.label} className="py-1.5 text-right tabular-nums text-zinc-200">{f(b)}</td>
      ))}
    </tr>
  );
  const num = (x: number | null, d = 1) => (x == null ? "—" : x.toFixed(d));
  return (
    <table className="w-full text-sm">
      <thead>
        <tr className="text-xs text-zinc-500">
          <th />
          {rows.map((b) => (
            <th key={b.label} className="pb-2 text-right font-medium">{b.label}</th>
          ))}
        </tr>
      </thead>
      <tbody className="divide-y divide-zinc-800/60">
        {metric("Sessions", (b) => String(b.sessions))}
        {metric("Success", (b) => pct(b.successRate))}
        {metric("Prompts / session", (b) => num(b.avgTurns))}
        {metric("Corrections / session", (b) => num(b.correctionsPerSession, 2))}
        {metric("Interruptions / session", (b) => num(b.interruptionsPerSession, 2))}
        {metric("Active minutes", (b) => num(b.avgActiveMinutes, 0))}
        {metric("Re-edit rate", (b) => pct(b.reEditRate))}
        {metric("Friction score", (b) => num(b.avgFriction, 2))}
      </tbody>
    </table>
  );
}
