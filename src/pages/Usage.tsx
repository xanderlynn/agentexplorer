import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Bar, BarChart, CartesianGrid, Line, LineChart, ReferenceLine, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api, type Source } from "../api";
import type { Drill } from "../App";
import { Card, Empty, ErrorNote, Legend, Loading, PageHeader, RANGE_OPTIONS, Rate, Segmented, Stat } from "../components/ui";
import { SOURCE_COLOR, SOURCE_LABEL, compact, money, pct, sourceLabel } from "../format";
import { chart } from "../theme";

const SOURCES = Object.keys(SOURCE_LABEL) as Source[];

export default function Usage({ days, setDays, drill }: { days: number; setDays: (d: number) => void; drill: (d: Drill) => void }) {
  const q = useQuery({ queryKey: ["usage", days], queryFn: () => api.usage(days) });
  // On a subscription, tokens (limits) matter more than dollars.
  const [unit, setUnit] = useState<"tokens" | "cost">("tokens");
  const header = (
    <PageHeader
      title="Usage"
      subtitle="Tokens and spend by tool and model. On a subscription, tokens decide when you hit limits; Claude Code costs are as recorded, Copilot's are API-equivalent estimates."
      actions={
        <>
          <Segmented options={[{ value: "tokens", label: "Tokens" }, { value: "cost", label: "Cost" }]} value={unit} onChange={setUnit} />
          <Segmented options={RANGE_OPTIONS} value={days} onChange={setDays} />
        </>
      }
    />
  );
  if (q.isLoading) return <>{header}<Loading /></>;
  if (q.error) return <>{header}<ErrorNote error={q.error} /></>;
  if (!q.data) return header;
  const u = q.data;
  if (u.byModel.length === 0) return <>{header}<Empty title="No sessions in this range" /></>;

  const total = u.daily.reduce((n, d) => n + Object.values(d.cost).reduce((a, b) => a + b, 0), 0);
  const perDay = u.daily.map((d) => ({ date: d.date, ...(unit === "cost" ? d.cost : d.tokens) }));
  const tokens = u.input + u.output + u.cacheRead + u.cacheWrite;
  const cacheRows = u.daily.map((d) => {
    const prompt = d.input + d.cacheRead + d.cacheWrite;
    return { date: d.date, hitRate: prompt ? d.cacheRead / prompt : null };
  });
  // Straight-line pace from today's cumulative spend to the month's end.
  const pacePerDay = u.monthCumulative.length ? u.monthToDateUsd / u.monthCumulative.length : 0;
  const monthRows = Array.from({ length: u.daysInMonth }, (_, i) => {
    const actual = u.monthCumulative[i]?.[1];
    return { day: i + 1, actual: actual ?? null, pace: (i + 1) * pacePerDay };
  });
  const overBudget = u.budgetUsd != null && u.projectedMonthUsd > u.budgetUsd;

  return (
    <>
      {header}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat label="Tokens in range" value={compact(tokens)} hint={`${compact(u.output)} output · ${compact(u.cacheRead)} from cache`} />
        <Stat label="Tokens per session" value={u.sessions ? compact(tokens / u.sessions) : "—"} n={u.sessions} />
        <Stat label="Tokens per shipped session" value={u.shipped ? compact(tokens / u.shipped) : "—"} n={u.shipped} hint="all tokens ÷ sessions you rated shipped" />
        <Stat label="Cache hit rate" value={pct(u.cacheHitRate)} hint="share of prompt tokens read from cache" />
        <Stat label="Spend in range" value={money(total, true)} hint={u.costPerSession != null ? `${money(u.costPerSession)} per session with a cost` : undefined} />
        <Stat
          label="This month"
          value={money(u.monthToDateUsd, true)}
          hint={
            u.budgetUsd != null ? (
              <span className={overBudget ? "text-amber-300" : undefined}>
                on pace for {money(u.projectedMonthUsd)} of {money(u.budgetUsd)} budget
              </span>
            ) : (
              `on pace for ${money(u.projectedMonthUsd)} · set a budget in Settings`
            )
          }
        />
        <Stat label="Spend per shipped session" value={u.shipped ? money(total / u.shipped, true) : "—"} n={u.shipped} />
        <Stat label="Saved by caching" value={money(u.cacheSavingsUsd, true)} hint="vs. full input price, Claude models" />
      </div>

      <div className="mt-6 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="This month's spend" action={<Legend items={[["Actual", chart.accent], ["Pace", chart.axis]]} />}>
          <div className="h-60">
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={monthRows} margin={{ left: -6, right: 8, top: 8 }}>
                <CartesianGrid stroke={chart.grid} vertical={false} />
                <XAxis dataKey="day" tick={{ fill: chart.axis, fontSize: 12 }} />
                <YAxis tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(v: number) => `$${Math.round(v)}`} />
                <Tooltip {...chart.tooltip} labelFormatter={(d) => `Day ${d}`} formatter={(v, name) => [money(Number(v)), name === "pace" ? "Pace" : "Spent so far"]} />
                {u.budgetUsd != null && (
                  <ReferenceLine y={u.budgetUsd} stroke="var(--color-amber-500)" strokeDasharray="4 3" label={{ value: "budget", fill: "var(--color-amber-500)", fontSize: 12, position: "insideTopLeft" }} />
                )}
                <Line type="linear" dataKey="pace" stroke={chart.axis} strokeDasharray="4 3" dot={false} />
                <Line type="monotone" dataKey="actual" stroke={chart.accent} strokeWidth={2} dot={false} connectNulls={false} />
              </LineChart>
            </ResponsiveContainer>
          </div>
        </Card>
        <Card title={unit === "cost" ? "Spend per day by tool" : "Tokens per day by tool"} action={<Legend items={SOURCES.map((s) => [SOURCE_LABEL[s], SOURCE_COLOR[s]])} />}>
          <div className="h-60">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart
                data={perDay}
                margin={{ left: -6, right: 8, top: 8 }}
                onClick={(e) => {
                  const date = e?.activeLabel as string | undefined;
                  if (date) drill({ label: `sessions on ${date}`, date });
                }}
              >
                <CartesianGrid stroke={chart.grid} vertical={false} />
                <XAxis dataKey="date" tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(d: string) => d.slice(5)} minTickGap={24} />
                <YAxis tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(v: number) => (unit === "cost" ? `$${v}` : compact(v))} />
                <Tooltip
                  {...chart.tooltip}
                  cursor={{ fill: chart.grid }}
                  formatter={(v, name) => [unit === "cost" ? money(Number(v)) : compact(Number(v)), sourceLabel(String(name))]}
                />
                {SOURCES.map((s) => (
                  <Bar key={s} dataKey={s} stackId="cost" fill={SOURCE_COLOR[s]} className="cursor-pointer" />
                ))}
              </BarChart>
            </ResponsiveContainer>
          </div>
        </Card>
        <Card title="Cache hit rate per day">
          <div className="h-52">
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={cacheRows} margin={{ left: -6, right: 8, top: 8 }}>
                <CartesianGrid stroke={chart.grid} vertical={false} />
                <XAxis dataKey="date" tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(d: string) => d.slice(5)} minTickGap={24} />
                <YAxis domain={[0, 1]} tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(v: number) => `${Math.round(v * 100)}%`} />
                <Tooltip {...chart.tooltip} formatter={(v) => [pct(Number(v)), "Hit rate"]} />
                <Line type="monotone" dataKey="hitRate" stroke="var(--color-sky-500)" strokeWidth={2} dot={false} connectNulls />
              </LineChart>
            </ResponsiveContainer>
          </div>
          <p className="mt-1 text-xs text-zinc-500">Higher is cheaper. Long pauses let the prompt cache expire.</p>
        </Card>
        <Card title="Tokens per day (excluding cache reads)" action={<Legend items={[["Input", chart.axis], ["Cache write", "var(--color-amber-500)"], ["Output", chart.accent]]} />}>
          <div className="h-52">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={u.daily} margin={{ left: -6, right: 8, top: 8 }}>
                <CartesianGrid stroke={chart.grid} vertical={false} />
                <XAxis dataKey="date" tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(d: string) => d.slice(5)} minTickGap={24} />
                <YAxis tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(v: number) => compact(v)} />
                <Tooltip {...chart.tooltip} cursor={{ fill: chart.grid }} formatter={(v) => compact(Number(v))} />
                <Bar dataKey="input" name="Input" stackId="t" fill={chart.axis} />
                <Bar dataKey="cacheWrite" name="Cache write" stackId="t" fill="var(--color-amber-500)" />
                <Bar dataKey="output" name="Output" stackId="t" fill={chart.accent} />
              </BarChart>
            </ResponsiveContainer>
          </div>
          <p className="mt-1 text-xs text-zinc-500">Cache reads ({compact(u.cacheRead)} in range) are cheap and would dwarf the rest, so they're shown as the hit rate instead.</p>
        </Card>
      </div>

      <Card title="Model comparison" className="mt-4">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-zinc-500">
                <th className="pb-2 font-medium">Model</th>
                <th className="pb-2 text-right font-medium">Sessions</th>
                <th className="pb-2 text-right font-medium">Spend</th>
                <th className="pb-2 text-right font-medium">Per session</th>
                <th className="pb-2 text-right font-medium">Tokens / session</th>
                <th className="pb-2 text-right font-medium">Prompts</th>
                <th className="pb-2 text-right font-medium">Active min</th>
                <th className="pb-2 text-right font-medium">Flow ≈</th>
                <th className="w-40 pb-2 pl-4 font-medium">Success (rated)</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800/60">
              {u.byModel.map((m) => (
                <tr key={m.model} className="cursor-pointer hover:bg-zinc-800/30" onClick={() => drill({ label: `model ${m.model}`, model: m.model })}>
                  <td className="py-1.5 font-mono text-xs text-zinc-200">{m.model}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{m.sessions}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{money(m.costUsd)}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{money(m.costPerSession)}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{compact(m.tokens / Math.max(1, m.sessions))}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{m.avgTurns.toFixed(1)}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{Math.round(m.avgActiveMinutes)}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{m.avgFriction.toFixed(1)}</td>
                  <td className="py-1.5 pl-4">
                    <Rate rate={m.successRate} n={m.rated} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <p className="mt-3 text-xs text-zinc-500">A session counts under the model it used most. Click a row for its sessions.</p>
      </Card>
    </>
  );
}
