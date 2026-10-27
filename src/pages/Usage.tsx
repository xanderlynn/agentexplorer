import { useQuery } from "@tanstack/react-query";
import { Area, AreaChart, Bar, BarChart, CartesianGrid, Legend, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api, type Source } from "../api";
import { Bar as Meter, Card, Empty, ErrorNote, Loading, PageHeader, RANGE_OPTIONS, Segmented, Stat } from "../components/ui";
import { SOURCE_COLOR, SOURCE_LABEL, compact, money, pct } from "../format";
import { chart } from "../theme";

const SOURCES = Object.keys(SOURCE_LABEL) as Source[];

export default function Usage({ days, setDays }: { days: number; setDays: (d: number) => void }) {
  const q = useQuery({ queryKey: ["usage", days], queryFn: () => api.usage(days) });
  const header = (
    <PageHeader
      title="Usage"
      subtitle="Spend, tokens, caching and models. Claude Code costs are as recorded; Copilot figures are API-equivalent estimates."
      actions={<Segmented options={RANGE_OPTIONS} value={days} onChange={setDays} />}
    />
  );
  if (q.isLoading) return <>{header}<Loading /></>;
  if (q.error) return <>{header}<ErrorNote error={q.error} /></>;
  if (!q.data) return header;
  const u = q.data;
  if (u.byModel.length === 0) return <>{header}<Empty title="No sessions in this range" /></>;

  const total = u.daily.reduce((n, d) => n + Object.values(d.cost).reduce((a, b) => a + b, 0), 0);
  const costRows = u.daily.map((d) => ({ date: d.date, ...d.cost }));
  const tokens = u.input + u.output + u.cacheRead + u.cacheWrite;

  return (
    <>
      {header}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat label="Spend in range" value={money(total, true)} hint={u.costPerSession != null ? `${money(u.costPerSession)} per session` : undefined} />
        <Stat label="This month" value={money(u.monthToDateUsd, true)} hint={`on pace for ${money(u.projectedMonthUsd)}`} />
        <Stat label="Cache hit rate" value={pct(u.cacheHitRate)} hint="share of prompt tokens read from cache" />
        <Stat label="Saved by caching" value={money(u.cacheSavingsUsd, true)} hint="vs. full input price, Claude models" />
        <Stat label="Tokens" value={compact(tokens)} hint={`${compact(u.output)} output`} />
        <Stat label="Input" value={compact(u.input)} />
        <Stat label="Cache reads" value={compact(u.cacheRead)} />
        <Stat label="Cache writes" value={compact(u.cacheWrite)} />
      </div>

      <div className="mt-6 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="Spend per day by tool">
          <div className="h-64">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={costRows} margin={{ left: -10, right: 8, top: 8 }}>
                <CartesianGrid stroke={chart.grid} vertical={false} />
                <XAxis dataKey="date" tick={{ fill: chart.axis, fontSize: 11 }} tickFormatter={(d: string) => d.slice(5)} minTickGap={24} />
                <YAxis tick={{ fill: chart.axis, fontSize: 11 }} tickFormatter={(v: number) => `$${v}`} />
                <Tooltip {...chart.tooltip} cursor={{ fill: chart.grid }} formatter={(v, name) => [money(Number(v)), SOURCE_LABEL[name as Source] ?? name]} />
                <Legend wrapperStyle={{ fontSize: 12 }} formatter={(v) => SOURCE_LABEL[v as Source] ?? v} />
                {SOURCES.map((s) => (
                  <Bar key={s} dataKey={s} stackId="cost" fill={SOURCE_COLOR[s]} />
                ))}
              </BarChart>
            </ResponsiveContainer>
          </div>
        </Card>
        <Card title="Token mix per day">
          <div className="h-64">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={u.daily} margin={{ left: -10, right: 8, top: 8 }}>
                <CartesianGrid stroke={chart.grid} vertical={false} />
                <XAxis dataKey="date" tick={{ fill: chart.axis, fontSize: 11 }} tickFormatter={(d: string) => d.slice(5)} minTickGap={24} />
                <YAxis tick={{ fill: chart.axis, fontSize: 11 }} tickFormatter={(v: number) => compact(v)} />
                <Tooltip {...chart.tooltip} formatter={(v) => compact(Number(v))} />
                <Legend wrapperStyle={{ fontSize: 12 }} />
                <Area type="monotone" dataKey="cacheRead" name="Cache read" stackId="t" stroke="var(--color-sky-500)" fill="var(--color-sky-500)" fillOpacity={0.25} />
                <Area type="monotone" dataKey="cacheWrite" name="Cache write" stackId="t" stroke="var(--color-amber-500)" fill="var(--color-amber-500)" fillOpacity={0.25} />
                <Area type="monotone" dataKey="input" name="Input" stackId="t" stroke={chart.axis} fill={chart.axis} fillOpacity={0.25} />
                <Area type="monotone" dataKey="output" name="Output" stackId="t" stroke={chart.accent} fill={chart.accent} fillOpacity={0.35} />
              </AreaChart>
            </ResponsiveContainer>
          </div>
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
                <th className="pb-2 text-right font-medium">Prompts</th>
                <th className="pb-2 text-right font-medium">Active min</th>
                <th className="pb-2 text-right font-medium">Friction</th>
                <th className="w-32 pb-2 pl-4 font-medium">Success</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800/60">
              {u.byModel.map((m) => (
                <tr key={m.model}>
                  <td className="py-1.5 font-mono text-xs text-zinc-200">{m.model}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{m.sessions}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{money(m.costUsd)}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{money(m.costPerSession)}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{m.avgTurns.toFixed(1)}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{Math.round(m.avgActiveMinutes)}</td>
                  <td className="py-1.5 text-right tabular-nums text-zinc-300">{m.avgFriction.toFixed(1)}/5</td>
                  <td className="py-1.5 pl-4">
                    <div className="flex items-center gap-2">
                      <Meter value={m.successRate} />
                      <span className="w-9 text-right text-xs tabular-nums text-zinc-400">{m.rated ? pct(m.successRate) : "—"}</span>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <p className="mt-3 text-xs text-zinc-500">A session is counted under the model it used most. Friction: 5 means a clear goal, little correction and a tight scope.</p>
      </Card>
    </>
  );
}
