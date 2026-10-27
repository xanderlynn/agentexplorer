import { useQuery } from "@tanstack/react-query";
import { Bar, BarChart, CartesianGrid, Legend, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api } from "../api";
import { Card, Empty, ErrorNote, Loading, PageHeader, RANGE_OPTIONS, Segmented, SourceBadge, Stat } from "../components/ui";
import { compact, duration, money, pct, projectName } from "../format";
import { GroupTable, tooltipStyle } from "./Overview";

export default function Scorecard({ days, setDays }: { days: number; setDays: (d: number) => void }) {
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
  const s = q.data!;
  if (s.sessions === 0) return <>{header}<Empty title="No sessions in this range" /></>;

  return (
    <>
      {header}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat label="Sessions → commits" value={pct(s.commitRate)} hint={`${s.sessionsWithCommits} of ${s.sessionsInRepos} sessions in git repos`} />
        <Stat label="Commits linked" value={s.commits} hint={`${s.aiCommits} carry an AI co-author trailer`} />
        <Stat label="Lines changed" value={`+${compact(s.insertions)} −${compact(s.deletions)}`} />
        <Stat label="Shipped sessions" value={s.shipped} />
        <Stat label="Cost per commit" value={money(s.costPerCommit, true)} hint="Claude recorded + Copilot API-equivalent" />
        <Stat label="Active time per commit" value={s.activeMinutesPerCommit != null ? duration(s.activeMinutesPerCommit * 60000) : "—"} />
        <Stat label="Cost per shipped session" value={money(s.costPerShipped, true)} />
        <Stat label="Sessions" value={s.sessions} />
      </div>

      <Card title="Weekly output" className="mt-6">
        <div className="h-64">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={s.weekly} margin={{ left: -20, right: 8, top: 8 }}>
              <CartesianGrid stroke="#27272a" vertical={false} />
              <XAxis dataKey="weekStart" tick={{ fill: "#71717a", fontSize: 11 }} tickFormatter={(d: string) => d.slice(5)} />
              <YAxis allowDecimals={false} tick={{ fill: "#71717a", fontSize: 11 }} />
              <Tooltip {...tooltipStyle} cursor={{ fill: "#27272a" }} labelFormatter={(l) => `Week of ${l}`} />
              <Legend wrapperStyle={{ fontSize: 12, color: "#a1a1aa" }} />
              <Bar dataKey="sessions" name="Sessions" fill="#3f3f46" radius={[3, 3, 0, 0]} />
              <Bar dataKey="sessionsWithCommits" name="Sessions with commits" fill="#5b9df5" radius={[3, 3, 0, 0]} />
              <Bar dataKey="shipped" name="Shipped" fill="#10b981" radius={[3, 3, 0, 0]} />
            </BarChart>
          </ResponsiveContainer>
        </div>
      </Card>

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card title="By tool">
          <GroupTable rows={s.bySource} label={(k) => <SourceBadge source={k} />} />
        </Card>
        <Card title="By repository">
          {s.byProject.length === 0 ? (
            <p className="text-sm text-zinc-500">No sessions ran inside a git repository.</p>
          ) : (
            <GroupTable rows={s.byProject} label={(k) => <span className="text-zinc-200" title={k}>{projectName(k)}</span>} />
          )}
        </Card>
      </div>
      <p className="mt-4 text-xs text-zinc-600">
        A commit is linked to a session when you authored it in the same repository while the session was active or within 30 minutes after.
      </p>
    </>
  );
}
