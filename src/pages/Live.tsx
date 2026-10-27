import { useQuery } from "@tanstack/react-query";
import { api, type LiveSession, type LiveState } from "../api";
import { Card, Empty, ErrorNote, Loading, PageHeader, SourceBadge, Stat, cx } from "../components/ui";
import { ago, duration, projectName } from "../format";

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
  const q = useQuery({ queryKey: ["live"], queryFn: api.live, refetchInterval: 5000, staleTime: 0 });
  const header = <PageHeader title="Live" subtitle="Sessions active in the last 3 hours. The bottleneck is often the human; this shows when it's you." />;
  if (q.isLoading) return <>{header}<Loading /></>;
  if (q.error) return <>{header}<ErrorNote error={q.error} /></>;
  const d = q.data!;
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
            {sorted.map((s) => <Row key={s.id} s={s} onOpen={onOpen} />)}
          </ul>
        )}
      </Card>
    </>
  );
}

function Row({ s, onOpen }: { s: LiveSession; onOpen: (id: string) => void }) {
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
          </div>
        </div>
        <div className="shrink-0 text-right">
          <div className={cx("text-sm font-medium", st.text)}>{st.label}</div>
          <div className="text-xs text-zinc-500">{s.state === "waiting" || s.state === "needs_you" ? `for ${duration(s.ageMs)}` : ago(s.ageMs)}</div>
        </div>
      </button>
    </li>
  );
}
