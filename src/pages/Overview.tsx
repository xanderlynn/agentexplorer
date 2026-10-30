import { useQuery } from "@tanstack/react-query";
import { Area, AreaChart, Bar, BarChart, CartesianGrid, Cell, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api, type Source } from "../api";
import type { Drill, Page } from "../App";
import { Card, Empty, ErrorNote, GroupTable, Legend, Loading, PageHeader, RANGE_OPTIONS, Segmented, SourceBadge, Stat } from "../components/ui";
import { InsightItem } from "../components/InsightItem";
import { chart } from "../theme";
import { SOURCE_COLOR, compact, delta, duration, hours, money, pct, projectName, sourceLabel } from "../format";
import { profile } from "../stats";

function usualPct(series: (number | null)[]) {
  const p = profile(series);
  return p ? `usually ${pct(p.low)}–${pct(p.high)}, best ${pct(p.best)}` : undefined;
}

function usualNum(series: (number | null)[], fmt: (x: number) => string, higherIsBetter = true) {
  const p = profile(series, higherIsBetter);
  return p ? `usually ${fmt(p.low)}–${fmt(p.high)}, best ${fmt(p.best)}` : undefined;
}

export default function Overview({
  days,
  setDays,
  onOpen,
  go,
  drill,
}: {
  days: number;
  setDays: (d: number) => void;
  onOpen: (id: string) => void;
  go: (p: Page) => void;
  drill: (d: Drill) => void;
}) {
  const ov = useQuery({ queryKey: ["overview", days], queryFn: () => api.overview(days) });
  const ins = useQuery({ queryKey: ["insights", days], queryFn: () => api.insights(days) });
  const unrated = useQuery({ queryKey: ["sessions", { days, outcome: "unrated" }], queryFn: () => api.sessions({ days, outcome: "unrated" }) });
  const failures = useQuery({ queryKey: ["failures", days], queryFn: () => api.failureClusters(days) });

  const header = (
    <PageHeader
      title="Overview"
      subtitle="How your AI-assisted work is going, and where to improve."
      actions={<Segmented options={RANGE_OPTIONS} value={days} onChange={setDays} />}
    />
  );
  if (ov.isLoading) return <>{header}<Loading /></>;
  if (ov.error) return <>{header}<ErrorNote error={ov.error} /></>;
  if (!ov.data) return header;
  const o = ov.data;
  const t = o.totals;
  const p = o.previous;
  const tr = o.trend;

  if (t.sessions === 0) {
    return (
      <>
        {header}
        <Empty title="No sessions in this range yet">
          Sessions from Claude Code, Copilot CLI and Copilot Chat in VS Code appear here automatically. Check Settings to see which sources were found.
        </Empty>
      </>
    );
  }

  const daily = o.daily.map((d) => ({ ...d, notShipped: d.sessions - d.shipped }));

  return (
    <>
      {header}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat
          label="Success rate"
          value={t.rated ? pct(t.successRate) : "—"}
          n={`${t.rated} rated`}
          change={t.successRate != null && p?.successRate != null ? t.successRate - p.successRate : null}
          points
          spark={tr.successRate}
          usual={usualPct(tr.successRate)}
          onClick={() => drill({ label: "unrated sessions", outcome: "unrated" })}
          hint={t.rated < t.sessions ? `${t.sessions - t.rated} unrated: click to rate` : undefined}
        />
        <Stat
          label="Shipped"
          value={t.shipped}
          change={delta(t.shipped, p?.shipped)}
          spark={tr.shipped}
          usual={usualNum(tr.shipped, (x) => String(Math.round(x)))}
          onClick={() => drill({ label: "shipped sessions", outcome: "shipped" })}
        />
        <Stat label="Commits" value={t.commits} change={delta(t.commits, p?.commits)} spark={tr.commits} hint={`${t.aiCommits} with AI trailers`} />
        <Stat
          label="Agents waited on you"
          value={hours(t.waitHours)}
          change={delta(t.waitHours, p?.waitHours)}
          goodWhenUp={false}
          spark={tr.waitHours}
          hint={t.avgWaitSeconds != null ? `${duration(t.avgWaitSeconds * 1000)} typical response` : "no hand-backs yet"}
        />
        <Stat label="Sessions" value={t.sessions} change={delta(t.sessions, p?.sessions)} spark={tr.sessions} hint={`${hours(t.activeHours)} active`} />
        <Stat
          label="Interruptions / session"
          value={(t.interruptions / Math.max(1, t.sessions)).toFixed(1)}
          spark={tr.interruptionsPerSession}
          goodWhenUp={false}
          usual={usualNum(tr.interruptionsPerSession, (x) => x.toFixed(1), false)}
        />
        <Stat
          label="Tool error rate"
          value={pct(t.toolCalls ? t.toolErrors / t.toolCalls : null, 1)}
          spark={tr.toolErrorRate}
          goodWhenUp={false}
          hint={`${compact(t.toolCalls)} tool calls`}
        />
        <Stat label="Spend" value={money(t.costUsd, t.costHasEstimates)} spark={tr.costUsd} goodWhenUp={false} hint={`${compact(t.premiumRequests)} Copilot premium requests`} onClick={() => go("usage")} />
      </div>
      <p className="mt-2 text-xs text-zinc-500">Sparklines show the last 12 weeks; "usually" is your middle half of weeks.</p>

      <div className="mt-6 grid grid-cols-1 gap-4 xl:grid-cols-3">
        <Card title="Insights" className="xl:col-span-2" action={<button className="text-xs text-zinc-400 hover:text-zinc-200" onClick={() => go("retro")}>Get a full retro →</button>}>
          {ins.isLoading && <Loading />}
          {ins.data && ins.data.length === 0 && <p className="text-sm text-zinc-500">Not enough rated sessions to compare yet. Rate a few on the Sessions page.</p>}
          <ul className="space-y-2">
            {ins.data?.map((i) => (
              <InsightItem key={i.id} insight={i} onOpen={i.sessionIds.length ? () => drill({ label: i.title, ids: i.sessionIds }) : undefined} />
            ))}
          </ul>
        </Card>

        <Card title={`Unrated sessions (${unrated.data?.length ?? 0})`} action={<button className="text-xs text-zinc-400 hover:text-zinc-200" onClick={() => drill({ label: "unrated sessions", outcome: "unrated" })}>All unrated →</button>}>
          {unrated.data?.length === 0 && <p className="text-sm text-zinc-500">Everything is rated. Nice.</p>}
          <ul className="divide-y divide-zinc-800/70">
            {unrated.data?.slice(0, 7).map((s) => (
              <li key={s.id}>
                <button className="w-full py-2 text-left hover:bg-zinc-800/30" onClick={() => onOpen(s.id)}>
                  <div className="truncate text-sm text-zinc-200">{s.title ?? "Untitled"}</div>
                  <div className="mt-0.5 flex items-center gap-2 text-xs text-zinc-500">
                    <SourceBadge source={s.source} entrypoint={s.entrypoint} /> · {projectName(s.repoRoot ?? s.projectPath)} · {duration(s.activeMs)}
                  </div>
                </button>
              </li>
            ))}
          </ul>
        </Card>
      </div>

      {failures.data && failures.data.length > 0 && (
        <Card title="Why sessions go wrong" className="mt-4">
          <p className="mb-3 text-xs text-zinc-500">Abandoned, redone, or reworked after shipping, grouped by the clearest signal in each session.</p>
          <ul className="grid grid-cols-1 gap-2 md:grid-cols-2">
            {failures.data.map((f) => (
              <li key={f.cause}>
                <button className="w-full rounded-lg border border-zinc-800 p-3 text-left hover:bg-zinc-800/30" onClick={() => drill({ label: f.cause, ids: f.ids })}>
                  <div className="flex items-baseline justify-between">
                    <span className="text-sm font-medium text-zinc-100">{f.cause}</span>
                    <span className="text-sm tabular-nums text-zinc-300">{f.ids.length}</span>
                  </div>
                  <div className="mt-0.5 text-xs text-zinc-400">{f.advice}</div>
                </button>
              </li>
            ))}
          </ul>
        </Card>
      )}

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="Sessions per day" action={<Legend items={[["Shipped", chart.accent], ["Other", chart.muted]]} />}>
          <div className="h-56">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart
                data={daily}
                margin={{ left: -20, right: 8, top: 8 }}
                onClick={(e) => {
                  const date = e?.activeLabel as string | undefined;
                  if (date) drill({ label: `sessions on ${date}`, date });
                }}
              >
                <CartesianGrid stroke={chart.grid} vertical={false} />
                <XAxis dataKey="date" tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(d: string) => d.slice(5)} minTickGap={24} />
                <YAxis allowDecimals={false} tick={{ fill: chart.axis, fontSize: 12 }} />
                <Tooltip {...chart.tooltip} cursor={{ fill: chart.grid }} />
                <Bar dataKey="shipped" name="Shipped" stackId="s" fill={chart.accent} className="cursor-pointer" />
                <Bar dataKey="notShipped" name="Other sessions" stackId="s" fill={chart.muted} radius={[3, 3, 0, 0]} className="cursor-pointer" />
              </BarChart>
            </ResponsiveContainer>
          </div>
          <p className="mt-1 text-xs text-zinc-500">Click a day to see its sessions.</p>
        </Card>
        <Card title="Active time per day">
          <div className="h-56">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={o.daily} margin={{ left: -20, right: 8, top: 8 }}>
                <defs>
                  <linearGradient id="activeFill" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor={chart.accent} stopOpacity={0.35} />
                    <stop offset="100%" stopColor={chart.accent} stopOpacity={0} />
                  </linearGradient>
                </defs>
                <CartesianGrid stroke={chart.grid} vertical={false} />
                <XAxis dataKey="date" tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(d: string) => d.slice(5)} minTickGap={24} />
                <YAxis tick={{ fill: chart.axis, fontSize: 12 }} />
                <Tooltip {...chart.tooltip} formatter={(v) => [`${Math.round(Number(v))} min`, "Active"]} />
                <Area type="monotone" dataKey="activeMinutes" stroke={chart.accent} fill="url(#activeFill)" strokeWidth={2} />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </Card>
      </div>

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="By tool">
          <div className="mb-3 h-32">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={o.bySource} layout="vertical" margin={{ left: 10, right: 16 }}>
                <XAxis type="number" hide />
                <YAxis type="category" dataKey="key" width={95} tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(k: string) => sourceLabel(k)} />
                <Tooltip {...chart.tooltip} cursor={{ fill: chart.grid }} formatter={(v) => [`${Number(v).toFixed(1)}h`, "Active"]} />
                <Bar dataKey="activeHours" radius={[0, 4, 4, 0]}>
                  {o.bySource.map((g) => <Cell key={g.key} fill={SOURCE_COLOR[g.key as Source] ?? chart.muted} />)}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </div>
          <GroupTable rows={o.bySource} label={(k) => <SourceBadge source={k} />} onRow={(k) => drill({ label: sourceLabel(k), source: k as Source })} />
        </Card>
        <Card title="By project">
          <GroupTable
            rows={o.byProject}
            label={(k) => <span className="truncate text-zinc-200" title={k}>{projectName(k)}</span>}
            onRow={(k) => drill({ label: `project ${projectName(k)}`, project: k })}
          />
        </Card>
      </div>
    </>
  );
}
