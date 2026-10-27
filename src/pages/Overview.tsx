import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, Info } from "lucide-react";
import { Area, AreaChart, Bar, BarChart, CartesianGrid, Cell, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api, type GroupStat, type Insight, type Source } from "../api";
import type { Page } from "../App";
import { Bar as Meter, Card, Empty, ErrorNote, Loading, PageHeader, RANGE_OPTIONS, Segmented, SourceBadge, Stat } from "../components/ui";
import { chart } from "../theme";
import { SOURCE_COLOR, compact, delta, duration, hours, money, pct, projectName } from "../format";

export const tooltipStyle = chart.tooltip;

export default function Overview({ days, setDays, onOpen, go }: { days: number; setDays: (d: number) => void; onOpen: (id: string) => void; go: (p: Page) => void }) {
  const ov = useQuery({ queryKey: ["overview", days], queryFn: () => api.overview(days) });
  const ins = useQuery({ queryKey: ["insights", days], queryFn: () => api.insights(days) });
  const unrated = useQuery({ queryKey: ["sessions", { days, outcome: "unrated" }], queryFn: () => api.sessions({ days, outcome: "unrated" }) });

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

  return (
    <>
      {header}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat label="Sessions" value={t.sessions} change={delta(t.sessions, p?.sessions)} hint={`${hours(t.activeHours)} active`} />
        <Stat
          label="Success rate"
          value={pct(t.successRate)}
          change={t.successRate != null && p?.successRate != null ? t.successRate - p.successRate : null}
          points
          hint={`${t.rated} of ${t.sessions} rated`}
        />
        <Stat label="Commits" value={t.commits} change={delta(t.commits, p?.commits)} hint={`${t.aiCommits} with AI trailers`} />
        <Stat
          label="Agents waited on you"
          value={hours(t.waitHours)}
          change={delta(t.waitHours, p?.waitHours)}
          goodWhenUp={false}
          hint={t.avgWaitSeconds != null ? `${duration(t.avgWaitSeconds * 1000)} avg response` : "no hand-backs yet"}
        />
        <Stat label="Claude spend" value={money(o.bySource.find((g) => g.key === "claude_code")?.costUsd ?? 0)} hint="as recorded by Claude Code" />
        <Stat label="Copilot premium requests" value={compact(t.premiumRequests)} hint="Copilot CLI" />
        <Stat label="Interruptions" value={t.interruptions} change={delta(t.interruptions, p?.interruptions)} goodWhenUp={false} hint={`${(t.interruptions / Math.max(1, t.sessions)).toFixed(1)} per session`} />
        <Stat label="Tool error rate" value={pct(t.toolCalls ? t.toolErrors / t.toolCalls : null, 1)} hint={`${compact(t.toolCalls)} tool calls`} />
      </div>

      <div className="mt-6 grid grid-cols-1 gap-4 xl:grid-cols-3">
        <Card title="Insights" className="xl:col-span-2" action={<button className="text-xs text-zinc-500 hover:text-zinc-300" onClick={() => go("retro")}>Get a full retro →</button>}>
          {ins.isLoading && <Loading />}
          {ins.data && ins.data.length === 0 && <p className="text-sm text-zinc-500">Not enough rated sessions to compare yet. Rate a few on the Sessions page.</p>}
          <ul className="space-y-3">
            {ins.data?.map((i) => <InsightItem key={i.id} insight={i} />)}
          </ul>
        </Card>

        <Card title={`Unrated sessions (${unrated.data?.length ?? 0})`} action={<button className="text-xs text-zinc-500 hover:text-zinc-300" onClick={() => go("sessions")}>All sessions →</button>}>
          {unrated.data?.length === 0 && <p className="text-sm text-zinc-500">Everything is rated. Nice.</p>}
          <ul className="divide-y divide-zinc-800/70">
            {unrated.data?.slice(0, 7).map((s) => (
              <li key={s.id}>
                <button className="w-full py-2 text-left hover:bg-zinc-800/30" onClick={() => onOpen(s.id)}>
                  <div className="truncate text-sm text-zinc-200">{s.title ?? "Untitled"}</div>
                  <div className="mt-0.5 flex items-center gap-2 text-xs text-zinc-500">
                    <SourceBadge source={s.source} /> · {projectName(s.repoRoot ?? s.projectPath)} · {duration(s.activeMs)}
                  </div>
                </button>
              </li>
            ))}
          </ul>
        </Card>
      </div>

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
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
                <XAxis dataKey="date" tick={{ fill: chart.axis, fontSize: 11 }} tickFormatter={(d: string) => d.slice(5)} minTickGap={24} />
                <YAxis tick={{ fill: chart.axis, fontSize: 11 }} />
                <Tooltip {...tooltipStyle} formatter={(v) => [`${Math.round(Number(v))} min`, "Active"]} />
                <Area type="monotone" dataKey="activeMinutes" stroke={chart.accent} fill="url(#activeFill)" strokeWidth={2} />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </Card>
        <Card title="Sessions and shipped per day">
          <div className="h-56">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={o.daily} margin={{ left: -20, right: 8, top: 8 }}>
                <CartesianGrid stroke={chart.grid} vertical={false} />
                <XAxis dataKey="date" tick={{ fill: chart.axis, fontSize: 11 }} tickFormatter={(d: string) => d.slice(5)} minTickGap={24} />
                <YAxis allowDecimals={false} tick={{ fill: chart.axis, fontSize: 11 }} />
                <Tooltip {...tooltipStyle} cursor={{ fill: chart.grid }} />
                <Bar dataKey="sessions" name="Sessions" fill={chart.muted} radius={[3, 3, 0, 0]} />
                <Bar dataKey="shipped" name="Shipped" fill={chart.accent} radius={[3, 3, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </Card>
      </div>

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="By tool">
          <div className="mb-3 h-36">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={o.bySource} layout="vertical" margin={{ left: 10, right: 16 }}>
                <XAxis type="number" hide />
                <YAxis type="category" dataKey="key" width={95} tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(k: string) => (k === "claude_code" ? "Claude Code" : k === "copilot_cli" ? "Copilot CLI" : "Copilot Chat")} />
                <Tooltip {...tooltipStyle} cursor={{ fill: chart.grid }} formatter={(v) => [`${Number(v).toFixed(1)}h`, "Active"]} />
                <Bar dataKey="activeHours" radius={[0, 4, 4, 0]}>
                  {o.bySource.map((g) => <Cell key={g.key} fill={SOURCE_COLOR[g.key as Source] ?? "#71717a"} />)}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </div>
          <GroupTable rows={o.bySource} label={(k) => <SourceBadge source={k} />} />
        </Card>
        <Card title="By project">
          <GroupTable rows={o.byProject} label={(k) => <span className="truncate text-zinc-200" title={k}>{projectName(k)}</span>} />
        </Card>
      </div>
    </>
  );
}

export function InsightItem({ insight }: { insight: Insight }) {
  const Icon = insight.kind === "warning" ? AlertTriangle : insight.kind === "positive" ? CheckCircle2 : Info;
  const color = insight.kind === "warning" ? "text-amber-400" : insight.kind === "positive" ? "text-emerald-400" : "text-sky-400";
  return (
    <li className="flex gap-3">
      <Icon size={18} className={`mt-0.5 shrink-0 ${color}`} />
      <div>
        <div className="text-sm font-medium text-zinc-100">{insight.title}</div>
        <div className="mt-0.5 text-sm text-zinc-400">{insight.detail}</div>
        <div className="mt-1 text-xs text-zinc-600">{insight.evidence}</div>
      </div>
    </li>
  );
}

export function GroupTable({ rows, label }: { rows: GroupStat[]; label: (key: string) => ReactNode }) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="text-left text-xs text-zinc-500">
            <th className="pb-2 font-medium" />
            <th className="pb-2 text-right font-medium">Sessions</th>
            <th className="pb-2 text-right font-medium">Active</th>
            <th className="pb-2 text-right font-medium">Commits</th>
            <th className="w-28 pb-2 pl-4 font-medium">Success</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-800/60">
          {rows.map((g) => (
            <tr key={g.key}>
              <td className="max-w-[14rem] py-1.5 pr-2">{label(g.key)}</td>
              <td className="py-1.5 text-right tabular-nums text-zinc-300">{g.sessions}</td>
              <td className="py-1.5 text-right tabular-nums text-zinc-300">{hours(g.activeHours)}</td>
              <td className="py-1.5 text-right tabular-nums text-zinc-300">{g.commits}</td>
              <td className="py-1.5 pl-4">
                <div className="flex items-center gap-2">
                  <Meter value={g.successRate} />
                  <span className="w-9 text-right text-xs tabular-nums text-zinc-400">{g.rated ? pct(g.successRate) : "—"}</span>
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
