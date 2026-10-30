import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, Bot, Circle, CircleAlert, CircleCheck, GitBranch, LoaderCircle, Lock, User, type LucideIcon } from "lucide-react";
import { api, type TaskKind, type Workstream, type WsTask } from "../api";
import { StatusBadge } from "../components/status";
import { Button, Card, Empty, ErrorNote, Loading, PageHeader, Segmented, SourceBadge, cx } from "../components/ui";
import { ago, projectName, roleOf } from "../format";
import { initials, progress, sortForYou, taskState, tree, type TaskState } from "../workstreams";

const STATE: Record<TaskState, { icon: LucideIcon; className: string; label: string }> = {
  done: { icon: CircleCheck, className: "text-emerald-400", label: "Done" },
  doing: { icon: LoaderCircle, className: "text-sky-400", label: "In progress" },
  yours: { icon: CircleAlert, className: "text-amber-400", label: "Needs you" },
  blocked: { icon: Lock, className: "text-zinc-500", label: "Waiting on another task" },
  todo: { icon: Circle, className: "text-zinc-600", label: "Not started" },
};

const agentLabel = (a: { name: string | null; role: string }) => a.name ?? (a.role === "lead" ? "Lead" : roleOf(a.role)[0]);

/**
 * Larger efforts that span sessions: one card each with progress, the team
 * on it and the latest update; open one for its task tree, which tasks are
 * agent or human work, and the agents' updates.
 */
export default function Workstreams({ onOpen }: { onOpen: (id: string) => void }) {
  const [days, setDays] = useState(30);
  const [filter, setFilter] = useState<"all" | "you">("all");
  // The open workstream is remembered, so it stays open across restarts.
  const [open, setOpenState] = useState<string | null>(() => {
    try {
      return localStorage.getItem("workstream");
    } catch {
      return null;
    }
  });
  const setOpen = (key: string | null) => {
    setOpenState(key);
    try {
      if (key) localStorage.setItem("workstream", key);
      else localStorage.removeItem("workstream");
    } catch {
      // Without storage it just isn't remembered.
    }
  };
  const q = useQuery({ queryKey: ["workstreams", days], queryFn: () => api.workstreams(days) });
  const list = useMemo(() => sortForYou(q.data ?? []).filter((w) => filter === "all" || w.counts.needsYou > 0), [q.data, filter]);
  const selected = q.data?.find((w) => w.key === open);

  if (selected) return <Detail ws={selected} onBack={() => setOpen(null)} onOpen={onOpen} />;

  const needYou = (q.data ?? []).filter((w) => w.counts.needsYou > 0).length;
  return (
    <>
      <PageHeader
        title="Workstreams"
        subtitle="Larger efforts across sessions: what's done, what's moving, and what's waiting on you."
        actions={
          <div className="flex flex-wrap items-center gap-2">
            <Segmented options={[{ value: "all" as const, label: "All" }, { value: "you" as const, label: `Needs you ${needYou}` }]} value={filter} onChange={setFilter} />
            <Segmented options={[{ value: 7, label: "7d" }, { value: 30, label: "30d" }, { value: 0, label: "All" }]} value={days} onChange={setDays} />
          </div>
        }
      />
      <ErrorNote error={q.error} />
      {q.isLoading ? (
        <Loading />
      ) : list.length === 0 ? (
        <Empty title={filter === "you" ? "Nothing is waiting on you" : "No workstreams yet"}>
          Work on a feature branch, or let an agent keep a task list (Claude Code's todos and plans, Copilot's todos), and its effort shows up here.
        </Empty>
      ) : (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {list.map((w) => (
            <EffortCard key={w.key} ws={w} onClick={() => setOpen(w.key)} />
          ))}
        </div>
      )}
      <div className="mt-4 flex flex-wrap gap-4 text-xs text-zinc-500">
        <span className="inline-flex items-center gap-1.5"><span className="h-1.5 w-3 rounded-sm bg-emerald-500" /> Done</span>
        <span className="inline-flex items-center gap-1.5"><span className="h-1.5 w-3 rounded-sm bg-sky-500" /> In progress</span>
        <span className="inline-flex items-center gap-1.5"><span className="h-1.5 w-3 rounded-sm bg-amber-400" /> Waiting on you</span>
      </div>
    </>
  );
}

function ProgressBar({ ws }: { ws: Workstream }) {
  const p = progress(ws);
  return (
    <div className="flex h-1.5 overflow-hidden rounded-full bg-zinc-800" role="img" aria-label={`${ws.counts.done} of ${p.total} tasks done`}>
      <div className="bg-emerald-500" style={{ width: `${p.done * 100}%` }} />
      <div className="bg-sky-500" style={{ width: `${p.doing * 100}%` }} />
      <div className="bg-amber-400" style={{ width: `${p.waiting * 100}%` }} />
    </div>
  );
}

function Avatars({ ws, max = 5 }: { ws: Workstream; max?: number }) {
  const shown = ws.agents.slice(0, max);
  return (
    <div className="flex items-center pl-1.5">
      {shown.map((a) => (
        <span
          key={`${a.sessionId}:${a.id}`}
          title={`${agentLabel(a)}${a.active ? " · working" : ""}`}
          className={cx(
            "-ml-1.5 inline-flex h-6 w-6 items-center justify-center rounded-full border-2 border-zinc-900 text-[10px] font-medium",
            a.role === "lead" ? "bg-emerald-500/20 text-emerald-200" : "bg-zinc-700 text-zinc-200",
            a.active && "ring-1 ring-sky-400",
          )}
        >
          {a.role === "lead" ? "L" : initials(agentLabel(a))}
        </span>
      ))}
      {ws.agents.length > max && <span className="ml-1.5 text-xs text-zinc-500">+{ws.agents.length - max}</span>}
    </div>
  );
}

function EffortCard({ ws, onClick }: { ws: Workstream; onClick: () => void }) {
  const latest = ws.updates[0];
  const subs = ws.agents.filter((a) => a.role !== "lead").length;
  const allDone = ws.tasks.length > 0 && ws.counts.done === ws.tasks.length;
  return (
    <button
      onClick={onClick}
      className="flex w-full min-w-0 flex-col items-stretch gap-3 rounded-xl border border-zinc-800 bg-zinc-900/60 p-4 text-left transition-colors hover:border-zinc-700 hover:bg-zinc-900 focus-visible:outline focus-visible:outline-2 focus-visible:outline-emerald-500"
    >
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-medium text-zinc-100">{ws.title}</div>
          <div className="truncate text-xs text-zinc-500">
            {ws.repo ? projectName(ws.repo) : "No repository"}
            {ws.branch && ` · ${ws.branch}`} · {ws.sessions.length} {ws.sessions.length === 1 ? "session" : "sessions"}
          </div>
        </div>
        {ws.counts.needsYou > 0 ? (
          <span className="shrink-0 rounded-full bg-amber-500/15 px-2 py-0.5 text-[11px] text-amber-300">{ws.counts.needsYou} need you</span>
        ) : (
          <span className="shrink-0 rounded-full bg-emerald-500/15 px-2 py-0.5 text-[11px] text-emerald-300">{allDone ? "Done" : "On track"}</span>
        )}
      </div>
      <ProgressBar ws={ws} />
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-zinc-400">
        <span>{ws.tasks.length ? `${ws.counts.done} of ${ws.tasks.length} done` : "No task list"}</span>
        <span className="inline-flex items-center gap-1"><Bot size={12} aria-hidden /> {ws.counts.agent} agent</span>
        <span className="inline-flex items-center gap-1"><User size={12} aria-hidden /> {ws.counts.human} human</span>
      </div>
      <div className="flex items-center justify-between gap-2">
        <Avatars ws={ws} />
        <span className="text-xs text-zinc-500">{subs ? `Lead + ${subs} sub-agent${subs === 1 ? "" : "s"}` : `${ws.sessions.length > 1 ? `${ws.sessions.length} leads` : "Lead"}`}</span>
      </div>
      <div className="border-t border-zinc-800 pt-2 text-sm">
        {latest ? (
          <>
            <div className="text-xs text-zinc-500">
              {latest.agent === "lead" ? "Lead" : latest.agent} · {latest.at ? ago(Date.now() - latest.at) : "earlier"}
            </div>
            <div className="line-clamp-2 text-zinc-300">{latest.text}</div>
          </>
        ) : (
          <div className="text-xs text-zinc-500">No updates yet. Last active {ago(Date.now() - ws.lastActivity)}.</div>
        )}
      </div>
    </button>
  );
}

function Detail({ ws, onBack, onOpen }: { ws: Workstream; onBack: () => void; onOpen: (id: string) => void }) {
  const [sel, setSel] = useState<string | null>(ws.tasks.find((t) => t.status === "in_progress")?.key ?? null);
  const task = ws.tasks.find((t) => t.key === sel) ?? null;
  const titleOf = (key: string) => ws.tasks.find((t) => t.key === key)?.title ?? key;
  const updates = task ? ws.updates.filter((u) => u.taskKey === task.key) : ws.updates;
  return (
    <>
      <div className="mb-4 flex items-start gap-3">
        <Button variant="ghost" size="sm" onClick={onBack} aria-label="All workstreams">
          <ArrowLeft size={16} />
        </Button>
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-lg font-semibold text-zinc-50">{ws.title}</h1>
          <div className="flex flex-wrap items-center gap-x-3 text-xs text-zinc-500">
            {ws.repo && <span>{projectName(ws.repo)}</span>}
            {ws.branch && <span className="inline-flex items-center gap-1"><GitBranch size={12} aria-hidden /> {ws.branch}</span>}
            <span>{ws.tasks.length ? `${ws.counts.done} of ${ws.tasks.length} done` : "No task list"}</span>
            {ws.counts.needsYou > 0 && <span className="text-amber-300">{ws.counts.needsYou} need you</span>}
          </div>
          <div className="mt-2 max-w-md"><ProgressBar ws={ws} /></div>
        </div>
        <Avatars ws={ws} max={8} />
      </div>

      <div className="grid gap-4 lg:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)]">
        <Card title="Tasks">
          <div className="space-y-4" role="tree" aria-label="Tasks by session">
            {tree(ws).map(({ session, tasks }) => (
              <div key={session.id} role="group">
                <div className="mb-1 flex items-center gap-2">
                  <span className="truncate text-sm font-medium text-zinc-200">{session.title ?? "Untitled session"}</span>
                  <StatusBadge status={session.status} size="xs" />
                  <SourceBadge source={session.source} />
                  <button className="ml-auto shrink-0 text-xs text-zinc-500 hover:text-zinc-200" onClick={() => onOpen(session.id)}>
                    Open
                  </button>
                </div>
                {tasks.length === 0 && <div className="pl-6 text-xs text-zinc-500">No task list in this session.</div>}
                {tasks.map((t) => (
                  <TaskRow key={t.key} t={t} selected={t.key === sel} onSelect={() => setSel(t.key === sel ? null : t.key)} />
                ))}
              </div>
            ))}
          </div>
        </Card>

        <div className="space-y-4">
          {task && <TaskDetail t={task} titleOf={titleOf} />}
          <Card title={task ? "Updates on this task" : "Updates"}>
            {updates.length === 0 ? (
              <p className="text-sm text-zinc-500">No updates yet.</p>
            ) : (
              <ol className="space-y-3">
                {updates.slice(0, 20).map((u, i) => (
                  <li key={i} className={cx("border-l-2 pl-3 text-sm", i === 0 ? "border-sky-500" : "border-zinc-700")}>
                    <div className="text-xs text-zinc-500">
                      {u.agent === "lead" ? "Lead" : u.agent} · {u.at ? ago(Date.now() - u.at) : "earlier"}
                    </div>
                    <div className="text-zinc-300">{u.text}</div>
                  </li>
                ))}
              </ol>
            )}
          </Card>
        </div>
      </div>
    </>
  );
}

function TaskRow({ t, selected, onSelect }: { t: WsTask; selected: boolean; onSelect: () => void }) {
  const st = STATE[taskState(t)];
  const Icon = st.icon;
  return (
    <button
      role="treeitem"
      aria-selected={selected}
      onClick={onSelect}
      className={cx(
        "flex w-full items-center gap-2 rounded-md py-1.5 pl-6 pr-2 text-left text-sm transition-colors hover:bg-zinc-800/60",
        selected && "bg-zinc-800",
        t.status === "done" ? "text-zinc-400" : "text-zinc-200",
      )}
    >
      <Icon size={15} className={cx("shrink-0", st.className, taskState(t) === "doing" && "motion-safe:animate-spin [animation-duration:3s]")} aria-label={st.label} />
      <span className="min-w-0 flex-1 truncate">{t.title}</span>
      <KindBadge kind={t.kind} reason={t.reason} />
    </button>
  );
}

function KindBadge({ kind, reason }: { kind: TaskKind; reason: string | null }) {
  return kind === "human" ? (
    <span title={reason ?? undefined} className="inline-flex shrink-0 items-center gap-1 rounded-full bg-amber-500/15 px-2 py-0.5 text-[11px] text-amber-300">
      <User size={11} aria-hidden /> You
    </span>
  ) : (
    <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-zinc-800 px-2 py-0.5 text-[11px] text-zinc-400">
      <Bot size={11} aria-hidden /> Agent
    </span>
  );
}

function TaskDetail({ t, titleOf }: { t: WsTask; titleOf: (key: string) => string }) {
  const qc = useQueryClient();
  const set = useMutation({
    mutationFn: (kind: TaskKind | null) => api.setTaskKind(t.key, kind),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["workstreams"] }),
  });
  const st = STATE[taskState(t)];
  return (
    <Card title="Task">
      <div className="text-sm font-medium text-zinc-100">{t.title}</div>
      <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-sm">
        <dt className="text-zinc-500">Status</dt>
        <dd className={st.className}>{st.label}</dd>
        <dt className="text-zinc-500">For</dt>
        <dd className="text-zinc-300">
          {t.kind === "human" ? "You" : "An agent"}
          {t.reason && <span className="text-zinc-500"> · {t.reason}</span>}
        </dd>
        {t.agent && (
          <>
            <dt className="text-zinc-500">Worked on by</dt>
            <dd className="text-zinc-300">{t.agent === "lead" ? "Lead" : t.agent}</dd>
          </>
        )}
        {t.blockedBy.length > 0 && (
          <>
            <dt className="text-zinc-500">Waits for</dt>
            <dd className="text-zinc-300">{t.blockedBy.map(titleOf).join(", ")}</dd>
          </>
        )}
        {t.updatedAt && (
          <>
            <dt className="text-zinc-500">Updated</dt>
            <dd className="text-zinc-300">{ago(Date.now() - t.updatedAt)}</dd>
          </>
        )}
      </dl>
      <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-zinc-800 pt-3">
        <span className="text-xs text-zinc-500">Who should do it?</span>
        <Button size="sm" variant={t.kind === "agent" ? "primary" : "secondary"} busy={set.isPending} onClick={() => set.mutate("agent")}>
          <Bot size={13} /> Agent
        </Button>
        <Button size="sm" variant={t.kind === "human" ? "primary" : "secondary"} busy={set.isPending} onClick={() => set.mutate("human")}>
          <User size={13} /> You
        </Button>
        {t.reason === "Set by you" && (
          <Button size="sm" variant="ghost" onClick={() => set.mutate(null)}>
            Reset
          </Button>
        )}
      </div>
      <ErrorNote error={set.error} />
    </Card>
  );
}
