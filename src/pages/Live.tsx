import { useQuery } from "@tanstack/react-query";
import { api, isWaiting, useLive, type LiveSession, type LiveState } from "../api";
import { roleOf } from "../components/TeamView";
import { Card, Empty, ErrorNote, Loading, PageHeader, SourceBadge, Stat, cx } from "../components/ui";
import { MODE_COLOR, MODE_LABEL, SMART_ZONE_DEFAULT, ago, compact, duration, projectName } from "../format";

const STATE: Record<LiveState, { label: string; dot: string; text: string; help: string }> = {
  needs_you: { label: "Needs permission", dot: "bg-rose-400 animate-pulse", text: "text-rose-300", help: "Waiting for you to approve a tool call." },
  waiting: { label: "Waiting on you", dot: "bg-amber-400 animate-pulse", text: "text-amber-300", help: "Finished its turn; your move." },
  working: { label: "Working", dot: "bg-emerald-400", text: "text-emerald-300", help: "Actively thinking or running tools." },
  quiet: { label: "Quiet", dot: "bg-sky-400", text: "text-sky-300", help: "Mid-turn but silent for a few minutes: a long command, or a permission prompt the tool doesn't log." },
  idle: { label: "Idle", dot: "bg-zinc-600", text: "text-zinc-400", help: "No recent activity." },
  ended: { label: "Ended", dot: "bg-zinc-700", text: "text-zinc-500", help: "The session process exited." },
};

const ORDER: LiveState[] = ["needs_you", "waiting", "quiet", "working", "idle", "ended"];

export default function Live({ onOpen }: { onOpen: (id: string) => void }) {
  // Shares the 5-second poll App.tsx runs for notifications.
  const q = useLive();
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const zone = Number(settings.data?.values["context.smartZoneTokens"]) || SMART_ZONE_DEFAULT;
  const header = <PageHeader title="Live" subtitle="Sessions active in the last 3 hours. The bottleneck is often the human; this shows when it's you." />;
  if (q.isLoading) return <>{header}<Loading /></>;
  if (q.error) return <>{header}<ErrorNote error={q.error} /></>;
  if (!q.data) return header;
  const d = q.data;
  const sorted = [...d.sessions].sort((a, b) => ORDER.indexOf(a.state) - ORDER.indexOf(b.state) || a.ageMs - b.ageMs);

  return (
    <>
      {header}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat label="Waiting on you" value={d.waitingOnYou} />
        <Stat label="Working" value={d.working} />
        <Stat label="Agents waited on you today" value={duration(d.waitMsToday)} />
        <Stat label="Recent sessions" value={d.sessions.length} />
      </div>
      <Card className="mt-6" title="Sessions">
        {sorted.length === 0 ? (
          <Empty title="Nothing running">Start a Claude Code or Copilot session and it will appear here within a few seconds.</Empty>
        ) : (
          <ul className="divide-y divide-zinc-800/70">
            {sorted.map((s) => <Row key={s.id} s={s} onOpen={onOpen} zone={zone} />)}
          </ul>
        )}
      </Card>
    </>
  );
}

function Row({ s, onOpen, zone }: { s: LiveSession; onOpen: (id: string) => void; zone: number }) {
  const st = STATE[s.state];
  return (
    <li>
      <button className="flex w-full items-center gap-4 py-3 text-left hover:bg-zinc-800/30" onClick={() => onOpen(s.id)}>
        <span className={cx("h-2.5 w-2.5 shrink-0 rounded-full", st.dot)} title={st.help} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm text-zinc-100">{s.title ?? "Untitled session"}</div>
          <div className="mt-0.5 flex flex-wrap items-center gap-x-2 text-xs text-zinc-500">
            <SourceBadge source={s.source} />
            <span>· {projectName(s.projectPath)}</span>
            {s.gitBranch && <span>· {s.gitBranch}</span>}
            <span>· {s.userTurns} prompts · {duration(s.activeMs)} active</span>
            {s.avgWaitMs != null && <span>· you respond in {duration(s.avgWaitMs)} on average</span>}
            {s.mode && (s.state === "working" || s.state === "quiet") && (
              <span className="inline-flex items-center gap-1">
                · <span className="h-2 w-2 rounded-full" style={{ background: MODE_COLOR[s.mode] }} aria-hidden /> {MODE_LABEL[s.mode].toLowerCase()}
              </span>
            )}
            {s.context > 0 && (
              <span className={s.context > zone ? "text-amber-300" : undefined} title={s.context > zone ? "Past the smart zone: consider starting fresh with a summary" : undefined}>
                · {compact(s.context)} context{s.context > zone ? " ⚠" : ""}
              </span>
            )}
          </div>
          {(s.agentsRunning.length > 0 || s.scheduled > 0) && (
            <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-xs">
              {s.agentsRunning.map(([who, task], i) => (
                <span key={i} className="inline-flex max-w-xs items-center gap-1 rounded-md bg-emerald-500/10 px-1.5 py-0.5 text-emerald-200" title={task}>
                  <span className="animate-pulse" aria-hidden>●</span>
                  <span className="truncate">{roleOf(who)[0]}: {task || "working"}</span>
                </span>
              ))}
              {s.agentsRunning.length > 0 && s.agentsTotal > s.agentsRunning.length && (
                <span className="text-zinc-500">{s.agentsTotal - s.agentsRunning.length} done</span>
              )}
              {s.scheduled > 0 && <span className="rounded-md bg-sky-500/10 px-1.5 py-0.5 text-sky-200">{s.scheduled} scheduled</span>}
            </div>
          )}
          {isWaiting(s.state) && s.lastMessage && (
            <div className="mt-1.5 line-clamp-2 rounded-md border-l-2 border-amber-500/60 bg-zinc-900/60 px-2 py-1 text-xs text-zinc-400">{s.lastMessage}</div>
          )}
        </div>
        <div className="shrink-0 text-right">
          <div className={cx("text-sm font-medium", st.text)}>{st.label}</div>
          <div className="text-xs text-zinc-500">{isWaiting(s.state) ? `for ${duration(s.ageMs)}` : ago(s.ageMs)}</div>
        </div>
      </button>
    </li>
  );
}
