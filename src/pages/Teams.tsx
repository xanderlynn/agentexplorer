import { useQuery } from "@tanstack/react-query";
import { CalendarClock } from "lucide-react";
import { api, type AgentStatus, type TeamSession } from "../api";
import { StatusDot } from "../components/status";
import { cronText, roleOf } from "../components/TeamView";
import { AGENT_STATUS, STATUS } from "../status";
import { Card, Empty, ErrorNote, Loading, PageHeader, SourceBadge, Stat, cx } from "../components/ui";
import { ago, compact, dateTime, duration, projectName } from "../format";

const isLive = (t: TeamSession) => t.team.live;

/**
 * Every session that worked as a team: who is at work right now, what is
 * scheduled, and recent teams with their roles. Each opens on its Team tab.
 */
export default function Teams({ onOpen }: { onOpen: (id: string) => void }) {
  const q = useQuery({ queryKey: ["teams"], queryFn: api.teams, refetchInterval: 10_000 });
  if (q.isLoading)
    return (
      <>
        <PageHeader title="Teams" />
        <Loading />
      </>
    );
  const teams = q.data ?? [];

  const working = teams.flatMap((t) => t.team.agents.filter((a) => a.status === "running").map((a) => ({ t, a })));
  const jobs = teams.filter(isLive).flatMap((t) =>
    t.team.scheduled.filter((j) => !j.cancelled && (j.recurring || (j.at != null && j.at > Date.now()))).map((j) => ({ t, j })),
  );
  const agents = teams.flatMap((t) => t.team.agents);
  const reported = agents.filter((a) => a.status === "done").length;
  const finished = agents.filter((a) => a.status !== "running").length;

  // How often each role is used and how often it reports back.
  const roles = new Map<string, { n: number; done: number; tokens: number }>();
  for (const a of agents) {
    const r = roleOf(a.role)[0];
    const x = roles.get(r) ?? { n: 0, done: 0, tokens: 0 };
    x.n += 1;
    x.done += a.status === "done" ? 1 : 0;
    x.tokens += a.tokens;
    roles.set(r, x);
  }

  return (
    <>
      <PageHeader title="Teams" subtitle="Sessions where the lead agent handed work to sub-agents: who is on each team, what they were asked to do, and what is scheduled." />
      <ErrorNote error={q.error} />
      {teams.length === 0 ? (
        <Empty title="No teams yet">When a Claude Code or Copilot session starts sub-agents or schedules work, it appears here.</Empty>
      ) : (
        <div className="space-y-6">
          <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
            <Stat label="Working now" value={working.length} />
            <Stat label="Scheduled" value={jobs.length} />
            <Stat label="Sessions with a team" value={teams.length} />
            <Stat label="Reported back" value={finished ? `${reported}/${finished}` : "—"} />
          </div>

          {(working.length > 0 || jobs.length > 0) && (
            <Card title="At work and coming up">
              <ul className="space-y-1.5">
                {working.map(({ t, a }) => (
                  <li key={`${t.id}-${a.id}`}>
                    <button className="flex w-full items-center gap-2 rounded-md px-1 py-1 text-left text-sm hover:bg-zinc-800/40" onClick={() => onOpen(t.id)}>
                      <StatusDot token={AGENT_STATUS.running} />
                      <span className="shrink-0 text-zinc-100">{a.name ?? roleOf(a.role)[0]}</span>
                      <span className="min-w-0 flex-1 truncate text-zinc-400">{a.task}</span>
                      <span className="shrink-0 text-xs text-zinc-500">
                        {t.title ?? projectName(t.projectPath)}
                        {a.startedAt && ` · ${duration(Date.now() - a.startedAt)}`}
                      </span>
                    </button>
                  </li>
                ))}
                {jobs.map(({ t, j }, i) => (
                  <li key={`${t.id}-job-${i}`}>
                    <button className="flex w-full items-center gap-2 rounded-md px-1 py-1 text-left text-sm hover:bg-zinc-800/40" onClick={() => onOpen(t.id)}>
                      <CalendarClock size={14} className={cx("shrink-0", STATUS.scheduled.text)} />
                      <span className="min-w-0 flex-1 truncate text-zinc-300">{j.task || "Scheduled job"}</span>
                      <span className="shrink-0 text-xs text-zinc-500">
                        {j.cron ? cronText(j.cron) : j.at ? `in ${duration(j.at - Date.now())}` : ""} · {t.title ?? projectName(t.projectPath)}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </Card>
          )}

          <Card title="Recent teams">
            <ul className="divide-y divide-zinc-800/70">
              {teams.map((t) => (
                <TeamRow key={t.id} t={t} onOpen={onOpen} />
              ))}
            </ul>
          </Card>

          {roles.size > 0 && (
            <Card title="Roles you use">
              <table className="w-full text-sm">
                <thead className="text-xs text-zinc-500">
                  <tr className="[&>th]:pb-2 [&>th]:font-normal">
                    <th className="text-left">Role</th>
                    <th className="text-right">Agents</th>
                    <th className="text-right">Reported back</th>
                    <th className="text-right">Tokens</th>
                  </tr>
                </thead>
                <tbody>
                  {[...roles.entries()]
                    .sort((a, b) => b[1].n - a[1].n)
                    .map(([role, x]) => (
                      <tr key={role} className="[&>td]:py-1.5">
                        <td className="text-zinc-200">{role}</td>
                        <td className="text-right tabular-nums text-zinc-300">{x.n}</td>
                        <td className="text-right tabular-nums text-zinc-300">{x.done}/{x.n}</td>
                        <td className="text-right tabular-nums text-zinc-400">{compact(x.tokens)}</td>
                      </tr>
                    ))}
                </tbody>
              </table>
            </Card>
          )}
        </div>
      )}
    </>
  );
}

function TeamRow({ t, onOpen }: { t: TeamSession; onOpen: (id: string) => void }) {
  const agents = t.team.agents;
  const roles = [...new Set(agents.map((a) => roleOf(a.role)[0]))];
  const tokens = agents.reduce((n, a) => n + a.tokens, 0);
  const counts = (s: AgentStatus) => agents.filter((a) => a.status === s).length;
  return (
    <li>
      <button className="flex w-full items-center gap-4 py-3 text-left hover:bg-zinc-800/30" onClick={() => onOpen(t.id)}>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm text-zinc-100">{t.title ?? "Untitled session"}</div>
          <div className="mt-0.5 flex flex-wrap items-center gap-x-2 text-xs text-zinc-500">
            <SourceBadge source={t.source} />
            <span>· {projectName(t.projectPath)}</span>
            <span>· {t.team.live ? `active ${ago(Date.now() - t.endedAt)}` : dateTime(t.startedAt)}</span>
            {roles.length > 0 && <span className="truncate">· {roles.join(", ")}</span>}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-0.5" aria-label={`${agents.length} agents`}>
          {agents.slice(0, 12).map((a) => (
            <span key={a.id} title={`${a.name ?? roleOf(a.role)[0]}: ${a.task}`}>
              <StatusDot token={AGENT_STATUS[a.status]} />
            </span>
          ))}
          {agents.length > 12 && <span className="ml-1 text-xs text-zinc-500">+{agents.length - 12}</span>}
        </div>
        <div className="w-36 shrink-0 text-right text-xs text-zinc-400">
          <div className="text-sm text-zinc-200">{agents.length} agent{agents.length === 1 ? "" : "s"}</div>
          {counts("running") > 0 && <span className={STATUS.working.text}>{counts("running")} working · </span>}
          {counts("failed") + counts("stopped") > 0 && <span className={STATUS.failed.text}>{counts("failed") + counts("stopped")} no report · </span>}
          {compact(tokens)} tokens
        </div>
      </button>
    </li>
  );
}
