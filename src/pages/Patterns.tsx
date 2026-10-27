import { useQuery } from "@tanstack/react-query";
import { Bar, BarChart, Line, LineChart, ReferenceLine, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api, type Benchmark, type Patterns as PatternsT } from "../api";
import type { Drill } from "../App";
import { Bar as Meter, Card, Empty, ErrorNote, Heuristic, Loading, OutcomeBadge, PageHeader, RANGE_OPTIONS, Rate, Segmented, SourceBadge } from "../components/ui";
import { duration, pct } from "../format";
import { quantile } from "../stats";
import { ToolHealthCard } from "../components/Reality";
import { chart, heat } from "../theme";

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

const FLOW_HELP =
  "Flow score: 5 means a clear opening prompt, little correction and a bounded session. It is a heuristic from the transcript, not a measurement of quality.";

export default function Patterns({
  days,
  setDays,
  onOpen,
  drill,
}: {
  days: number;
  setDays: (d: number) => void;
  onOpen: (id: string) => void;
  drill: (d: Drill) => void;
}) {
  const q = useQuery({ queryKey: ["patterns", days], queryFn: () => api.patterns(days) });
  const playbooks = useQuery({ queryKey: ["playbooks"], queryFn: api.playbooks });
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
  const marks = playbookMarks(playbooks.data ?? [], p.qualityWeekly.map((w) => w.weekStart));

  return (
    <>
      {header}
      <Card title="Activity over the last year" action={<HeatLegend label="active minutes" />}>
        <Calendar days={p.calendar} onDay={(date) => drill({ label: `sessions on ${date}`, date })} />
      </Card>

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="When you start sessions" action={<HeatLegend label="sessions" />}>
          <HourHeatmap grid={p.heatmap} onCell={(weekday, hour) => drill({ label: `${WEEKDAYS[weekday]}s at ${hour}:00`, weekday, hour })} />
        </Card>
        <Card
          title={
            <span>
              Flow score <span className="font-normal text-zinc-500">(5 = smooth)</span>
              <Heuristic title={FLOW_HELP} />
            </span>
          }
        >
          {p.friction ? (
            <div className="space-y-3">
              <FlowBar label="Clear ask" hint="goal, done-criteria and anchors in the opening prompt" value={p.friction.requirements} />
              <FlowBar label="Steering" hint="few corrections and interruptions" value={p.friction.steering} />
              <FlowBar label="Scope" hint="session kept to a bounded size" value={p.friction.scope} />
              <div className="border-t border-zinc-800 pt-3 text-sm text-zinc-300">
                Overall <span className="font-semibold text-zinc-50">{p.friction.overall.toFixed(1)}</span> / 5
              </div>
            </div>
          ) : (
            <p className="text-sm text-zinc-500">No sessions with signals in range.</p>
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
                  <th className="pb-2 text-right font-medium">Flow</th>
                  <th className="w-40 pb-2 pl-4 font-medium">Success (rated)</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-zinc-800/60">
                {p.categories.map((c) => (
                  <tr key={c.category} className="cursor-pointer hover:bg-zinc-800/30" onClick={() => drill({ label: `${c.category} sessions`, category: c.category })}>
                    <td className="py-1.5 capitalize text-zinc-200">{c.category}</td>
                    <td className="py-1.5 text-right tabular-nums text-zinc-300">{c.sessions}</td>
                    <td className="py-1.5 text-right tabular-nums text-zinc-300">{Math.round(c.avgActiveMinutes)}m</td>
                    <td className="py-1.5 text-right tabular-nums text-zinc-300">{c.avgFriction.toFixed(1)}</td>
                    <td className="py-1.5 pl-4">
                      <Rate rate={c.successRate} n={c.rated} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <p className="mt-2 text-xs text-zinc-500">Category is guessed from the opening prompt. Click a row for its sessions.</p>
        </Card>
        <Card title="What the agents do">
          <ToolMix p={p} />
          <div className="mt-3 h-52">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={p.tools} layout="vertical" margin={{ left: 10, right: 16 }}>
                <XAxis type="number" hide />
                <YAxis type="category" dataKey="tool" width={130} tick={{ fill: chart.axis, fontSize: 12 }} />
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
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-5">
            <Multiple title="Success" better="higher" data={p.qualityWeekly} k="successRate" marks={marks} fmt={(v) => pct(v)} domain={[0, 1]} />
            <Multiple title="Re-edit rate" better="lower" data={p.qualityWeekly} k="reEditRate" marks={marks} fmt={(v) => pct(v)} domain={[0, 1]} />
            <Multiple title="Blind-edit rate" better="lower" data={p.qualityWeekly} k="editsWithoutReadRate" marks={marks} fmt={(v) => pct(v)} domain={[0, 1]} />
            <Multiple title="Corrections / session" better="lower" data={p.qualityWeekly} k="correctionsPerSession" marks={marks} fmt={(v) => v.toFixed(1)} />
            <Multiple title="Flow score" better="higher" data={p.qualityWeekly} k="avgFriction" marks={marks} fmt={(v) => v.toFixed(1)} domain={[1, 5]} heuristic />
          </div>
        )}
        <p className="mt-3 text-xs text-zinc-500">
          Each panel has its own scale; the dashed line is the median week; dotted verticals mark weeks you changed an instruction file or prompt template. Re-edit: edits to a file already changed in the same session. Blind edit: editing a file the agent never read.
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

      <div className="mt-4">
        <ToolHealthCard days={days} />
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

type Week = PatternsT["qualityWeekly"][number];

/** One small chart per metric, so series with different units never share an axis. */
function Multiple({
  title,
  better,
  data,
  k,
  fmt,
  domain,
  heuristic,
  marks = [],
}: {
  title: string;
  better: "higher" | "lower";
  data: Week[];
  k: keyof Week;
  fmt: (v: number) => string;
  domain?: [number, number];
  heuristic?: boolean;
  marks?: { week: string; label: string }[];
}) {
  const values = data.map((w) => w[k]).filter((v): v is number => typeof v === "number");
  const median = quantile(values, 0.5);
  const last = values[values.length - 1];
  return (
    <div>
      <div className="flex items-baseline justify-between">
        <span className="text-sm text-zinc-200">
          {title}
          {heuristic && <Heuristic title={FLOW_HELP} />}
        </span>
        <span className="text-sm font-semibold tabular-nums text-zinc-100">{last != null ? fmt(last) : "—"}</span>
      </div>
      <div className="text-xs text-zinc-500">{better} is better</div>
      <div className="mt-1 h-24">
        <ResponsiveContainer width="100%" height="100%">
          <LineChart data={data} margin={{ left: 0, right: 4, top: 6, bottom: 0 }}>
            <XAxis dataKey="weekStart" hide />
            <YAxis hide domain={domain ?? ["auto", "auto"]} />
            <Tooltip {...chart.tooltip} labelFormatter={(l) => `Week of ${l}`} formatter={(v) => [fmt(Number(v)), title]} />
            {median != null && <ReferenceLine y={median} stroke={chart.axis} strokeDasharray="3 3" />}
            {marks.map((m) => (
              <ReferenceLine key={m.week + m.label} x={m.week} stroke="var(--color-sky-500)" strokeDasharray="1 3" />
            ))}
            <Line type="monotone" dataKey={k as string} stroke={chart.accent} strokeWidth={2} dot={{ r: 2 }} connectNulls />
          </LineChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}

/** Weeks (as Monday dates present in the chart) where a playbook changed. */
function playbookMarks(playbooks: import("../api").Playbook[], weeks: string[]) {
  const set = new Set(weeks);
  const monday = (ms: number) => {
    const d = new Date(ms);
    d.setHours(0, 0, 0, 0);
    d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  };
  const out: { week: string; label: string }[] = [];
  for (const p of playbooks) {
    // The oldest version is the starting point, not a change.
    for (const v of p.versions.slice(0, -1)) {
      const w = monday(v.lastSeen);
      if (set.has(w)) out.push({ week: w, label: p.name });
    }
  }
  return out;
}

function HeatLegend({ label }: { label: string }) {
  return (
    <span className="flex items-center gap-1.5 text-xs text-zinc-400">
      less
      {[0, 0.25, 0.5, 0.75, 1].map((x) => (
        <span key={x} className="h-3 w-3 rounded-sm" style={{ background: heat(x) }} />
      ))}
      more {label}
    </span>
  );
}

function Calendar({ days, onDay }: { days: PatternsT["calendar"]; onDay: (date: string) => void }) {
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
                <button
                  key={j}
                  className="h-3 w-3 rounded-sm outline-offset-1 hover:outline hover:outline-1 hover:outline-zinc-400"
                  style={{ background: heat(d.activeMinutes / max) }}
                  title={`${d.date}: ${d.sessions} sessions, ${Math.round(d.activeMinutes)} active min`}
                  aria-label={`${d.date}: ${d.sessions} sessions`}
                  onClick={() => d.sessions && onDay(d.date)}
                />
              ) : (
                <div key={j} className="h-3 w-3" />
              ),
            )}
          </div>
        ))}
      </div>
      <div className="mt-2 text-xs text-zinc-500">
        {total} sessions on {activeDays} days in the last year. Click a day to open its sessions.
      </div>
    </div>
  );
}

function HourHeatmap({ grid, onCell }: { grid: number[][]; onCell: (weekday: number, hour: number) => void }) {
  const max = Math.max(1, ...grid.flat());
  return (
    <div className="overflow-x-auto">
      <div className="inline-grid gap-[3px]" style={{ gridTemplateColumns: `2.5rem repeat(24, minmax(15px, 1fr))` }}>
        <div />
        {Array.from({ length: 24 }, (_, h) => (
          <div key={h} className="text-center text-xs text-zinc-500">{h % 3 === 0 ? h : ""}</div>
        ))}
        {grid.map((row, d) => (
          <div key={d} className="contents">
            <div className="text-xs leading-[15px] text-zinc-500">{WEEKDAYS[d]}</div>
            {row.map((n, h) => (
              <button
                key={h}
                className="h-[15px] rounded-sm hover:outline hover:outline-1 hover:outline-zinc-400"
                style={{ background: heat(n / max) }}
                title={`${WEEKDAYS[d]} ${h}:00: ${n} sessions`}
                aria-label={`${WEEKDAYS[d]} ${h}:00, ${n} sessions`}
                onClick={() => n && onCell(d, h)}
              />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function FlowBar({ label, hint, value }: { label: string; hint: string; value: number }) {
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
        <td key={b.label} className={`py-1.5 text-right tabular-nums ${b.sessions < 5 ? "text-zinc-500" : "text-zinc-200"}`}>{f(b)}</td>
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
            <th key={b.label} className="pb-2 text-right font-medium">
              {b.label}
              <div className="font-normal">n={b.sessions}</div>
            </th>
          ))}
        </tr>
      </thead>
      <tbody className="divide-y divide-zinc-800/60">
        {metric("Success", (b) => pct(b.successRate))}
        {metric("Prompts / session", (b) => num(b.avgTurns))}
        {metric("Corrections / session", (b) => num(b.correctionsPerSession, 2))}
        {metric("Interruptions / session", (b) => num(b.interruptionsPerSession, 2))}
        {metric("Active minutes", (b) => num(b.avgActiveMinutes, 0))}
        {metric("Re-edit rate", (b) => pct(b.reEditRate))}
        {metric("Flow score ≈", (b) => num(b.avgFriction, 1))}
      </tbody>
    </table>
  );
}
