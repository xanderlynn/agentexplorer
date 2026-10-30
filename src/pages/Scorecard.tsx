import { useQuery } from "@tanstack/react-query";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api, type Source } from "../api";
import { Card, Empty, ErrorNote, GroupTable, Legend, Loading, PageHeader, RANGE_OPTIONS, Segmented, SourceBadge, Stat } from "../components/ui";
import { compact, duration, money, pct, projectName } from "../format";
import type { Drill } from "../App";
import { FitMatrix, RealityCheck } from "../components/Reality";
import { chart } from "../theme";

export default function Scorecard({ days, setDays, drill }: { days: number; setDays: (d: number) => void; drill: (d: Drill) => void }) {
  const q = useQuery({ queryKey: ["scorecard", days], queryFn: () => api.scorecard(days) });
  const header = (
    <PageHeader
      title="Scorecard"
      subtitle="Output, not activity: how often a session turns into commits, and what each commit costs."
      actions={<Segmented options={RANGE_OPTIONS} value={days} onChange={setDays} />}
    />
  );
  if (q.isLoading) return <>{header}<Loading /></>;
  if (q.error) return <>{header}<ErrorNote error={q.error} /></>;
  if (!q.data) return header;
  const s = q.data;
  if (s.sessions === 0) return <>{header}<Empty title="No sessions in this range" /></>;

  return (
    <>
      {header}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat label="Sessions → commits" value={pct(s.commitRate)} hint={`${s.sessionsWithCommits} of ${s.sessionsInRepos} sessions in git repos`} />
        <Stat label="Commits linked" quiet="None linked. This tab needs sessions run in a git repository." value={s.commits} hint={`${s.aiCommits} carry an AI co-author trailer`} />
        <Stat label="Lines changed" value={`+${compact(s.insertions)} −${compact(s.deletions)}`} />
        <Stat label="Shipped sessions" value={s.shipped} />
        <Stat label="Cost per commit" value={money(s.costPerCommit, true)} hint="Claude recorded + Copilot API-equivalent" />
        <Stat label="Active time per commit" value={s.activeMinutesPerCommit != null ? duration(s.activeMinutesPerCommit * 60000) : "—"} />
        <Stat label="Cost per shipped session" value={money(s.costPerShipped, true)} />
        <Stat label="Sessions" value={s.sessions} />
      </div>

      <Card title="Weekly output" className="mt-6" action={<Legend items={[["Shipped", chart.accent], ["Committed, not shipped", "var(--color-sky-500)"], ["Other", chart.muted]]} />}>
        <div className="h-64">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart
              data={s.weekly.map((w) => ({
                ...w,
                committedNotShipped: Math.max(0, w.sessionsWithCommits - w.shipped),
                rest: Math.max(0, w.sessions - Math.max(w.sessionsWithCommits, w.shipped)),
              }))}
              margin={{ left: -20, right: 8, top: 8 }}
            >
              <CartesianGrid stroke={chart.grid} vertical={false} />
              <XAxis dataKey="weekStart" tick={{ fill: chart.axis, fontSize: 12 }} tickFormatter={(d: string) => d.slice(5)} />
              <YAxis allowDecimals={false} tick={{ fill: chart.axis, fontSize: 12 }} />
              <Tooltip {...chart.tooltip} cursor={{ fill: chart.grid }} labelFormatter={(l) => `Week of ${l}`} />
              <Bar isAnimationActive={false} dataKey="shipped" name="Shipped" stackId="w" fill={chart.accent} />
              <Bar isAnimationActive={false} dataKey="committedNotShipped" name="Committed, not shipped" stackId="w" fill="var(--color-sky-500)" />
              <Bar isAnimationActive={false} dataKey="rest" name="Other sessions" stackId="w" fill={chart.muted} radius={[3, 3, 0, 0]} />
            </BarChart>
          </ResponsiveContainer>
        </div>
        <p className="mt-1 text-xs text-zinc-500">Each bar is a week's sessions, split by how far they got.</p>
      </Card>

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="By tool">
          <GroupTable rows={s.bySource} label={(k) => <SourceBadge source={k} />} onRow={(k) => drill({ label: k, source: k as Source })} />
        </Card>
        <Card title="By repository">
          {s.byProject.length === 0 ? (
            <p className="text-sm text-zinc-500">No sessions ran inside a git repository.</p>
          ) : (
            <GroupTable rows={s.byProject} label={(k) => <span className="text-zinc-200" title={k}>{projectName(k)}</span>} onRow={(k) => drill({ label: `project ${projectName(k)}`, project: k })} />
          )}
        </Card>
      </div>
      <RealityCheck days={days} drill={drill} />
      <FitMatrix days={days} drill={drill} />
      <p className="mt-4 text-xs text-zinc-500">
        A commit is linked to a session when you authored it in the same repository while the session was active or within 30 minutes after.
      </p>
    </>
  );
}
