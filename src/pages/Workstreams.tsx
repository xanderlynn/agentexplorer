import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  Bot,
  Circle,
  CircleAlert,
  CircleCheck,
  Download,
  ExternalLink,
  GitBranch,
  LoaderCircle,
  Lock,
  Pencil,
  Plus,
  RefreshCw,
  Star,
  Trash2,
  User,
  X,
  type LucideIcon,
} from "lucide-react";
import { api, type TaskKind, type Workstream, type WsTask } from "../api";
import { StatusBadge } from "../components/status";
import { Button, Card, Empty, ErrorNote, Loading, PageHeader, Segmented, SourceBadge, cx, inputClass } from "../components/ui";
import { ago, projectName, roleOf } from "../format";
import { initials, progress, sortForYou, taskState, trackerName, trackerTasks, tree, type TaskState } from "../workstreams";

const STATE: Record<TaskState, { icon: LucideIcon; className: string; label: string }> = {
  done: { icon: CircleCheck, className: "text-emerald-400", label: "Done" },
  doing: { icon: LoaderCircle, className: "text-sky-400", label: "In progress" },
  yours: { icon: CircleAlert, className: "text-amber-400", label: "Needs you" },
  blocked: { icon: Lock, className: "text-zinc-500", label: "Blocked" },
  todo: { icon: Circle, className: "text-zinc-600", label: "Not started" },
};

/** Imported workstreams fetch their epic again when opened after this long. */
const SYNC_AFTER_MS = 15 * 60_000;

/** A change to your workstreams; the list reloads after it. */
function useWsChange<A>(fn: (arg: A) => Promise<unknown>) {
  const qc = useQueryClient();
  return useMutation({ mutationFn: fn, onSuccess: () => qc.invalidateQueries({ queryKey: ["workstreams"] }) });
}

const agentLabel = (a: { name: string | null; role: string }) => a.name ?? (a.role === "lead" ? "Lead" : roleOf(a.role)[0]);

/**
 * Larger efforts that span sessions: one card each with progress, the team
 * on it and the latest update; open one for its task tree, which tasks are
 * agent or human work, and the agents' updates.
 */
export default function Workstreams({ onOpen }: { onOpen: (id: string) => void }) {
  const [days, setDays] = useState(30);
  const [filter, setFilter] = useState<"all" | "starred" | "you">("all");
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
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
  const list = useMemo(
    () => sortForYou(q.data ?? []).filter((w) => filter === "all" || (filter === "starred" ? w.starred : w.counts.needsYou > 0)),
    [q.data, filter],
  );
  const create = useWsChange(({ title, ids }: { title: string; ids: string[] }) => api.createWorkstream(title, ids));
  const selected = q.data?.find((w) => w.key === open);

  if (selected) return <Detail ws={selected} onBack={() => setOpen(null)} onOpen={onOpen} />;

  const needYou = (q.data ?? []).filter((w) => w.counts.needsYou > 0).length;
  const starred = (q.data ?? []).filter((w) => w.starred).length;
  return (
    <>
      <PageHeader
        title="Workstreams"
        subtitle="Larger efforts across sessions: what's done, what's moving, and what's waiting on you."
        actions={
          <div className="flex flex-wrap items-center gap-2">
            <Segmented
              options={[
                { value: "all" as const, label: "All" },
                { value: "starred" as const, label: `Starred ${starred}` },
                { value: "you" as const, label: `Needs you ${needYou}` },
              ]}
              value={filter}
              onChange={setFilter}
            />
            <Segmented options={[{ value: 7, label: "7d" }, { value: 30, label: "30d" }, { value: 0, label: "All" }]} value={days} onChange={setDays} />
            <Button size="sm" onClick={() => setImporting(true)}>
              <Download size={14} /> Import epic
            </Button>
            <Button variant="primary" size="sm" onClick={() => setCreating(true)}>
              <Plus size={14} /> New workstream
            </Button>
          </div>
        }
      />
      {importing && (
        <ImportDialog
          onClose={() => setImporting(false)}
          onDone={(key) => {
            setImporting(false);
            setOpen(key);
          }}
        />
      )}
      {creating && (
        <SessionPicker
          heading="New workstream"
          named
          action="Create"
          busy={create.isPending}
          error={create.error}
          onClose={() => setCreating(false)}
          onDone={(ids, title) =>
            create.mutate(
              { title, ids },
              {
                onSuccess: (key) => {
                  setCreating(false);
                  setOpen(key as string);
                },
              },
            )
          }
        />
      )}
      <ErrorNote error={q.error} />
      {q.isLoading ? (
        <Loading />
      ) : list.length === 0 ? (
        <Empty title={filter === "you" ? "Nothing is waiting on you" : filter === "starred" ? "Nothing starred yet" : "No workstreams yet"}>
          {filter === "starred"
            ? "Star a workstream to keep it here, whatever its age."
            : "Work on a feature branch, or let an agent keep a task list (Claude Code's todos and plans, Copilot's todos), and its effort shows up here. Or make your own with New workstream, or import a GitHub or Azure DevOps epic."}
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

function StarButton({ ws, className }: { ws: Workstream; className?: string }) {
  const star = useWsChange((on: boolean) => api.starWorkstream(ws, on));
  const on = star.isPending ? star.variables : ws.starred;
  return (
    <button
      onClick={() => star.mutate(!ws.starred)}
      aria-pressed={on}
      aria-label={on ? "Unstar" : "Star"}
      title={on ? "Starred: kept here whatever its age" : "Star to keep it here"}
      className={cx("rounded-md p-1 transition-colors hover:bg-zinc-800", on ? "text-amber-300" : "text-zinc-500 hover:text-zinc-200", className)}
    >
      <Star size={15} fill={on ? "currentColor" : "none"} />
    </button>
  );
}

function EffortCard({ ws, onClick }: { ws: Workstream; onClick: () => void }) {
  const latest = ws.updates[0];
  const subs = ws.agents.filter((a) => a.role !== "lead").length;
  const allDone = ws.tasks.length > 0 && ws.counts.done === ws.tasks.length;
  return (
    <div className="relative min-w-0">
      <button
        onClick={onClick}
        className={cx(
          "flex h-full w-full min-w-0 flex-col items-stretch gap-3 rounded-xl border bg-zinc-900/60 p-4 text-left transition-colors hover:border-zinc-700 hover:bg-zinc-900 focus-visible:outline focus-visible:outline-2 focus-visible:outline-emerald-500",
          ws.starred ? "border-amber-500/30" : "border-zinc-800",
        )}
      >
        <div className="flex items-start justify-between gap-2 pr-7">
          <div className="min-w-0">
            <div className="truncate text-sm font-medium text-zinc-100">{ws.title}</div>
            <div className="truncate text-xs text-zinc-500">
              {ws.sourceUrl ? `${trackerName(ws.sourceUrl)} epic` : ws.repo ? projectName(ws.repo) : ws.custom ? "Your workstream" : "No repository"}
              {!ws.sourceUrl && ws.branch && ` · ${ws.branch}`} · {ws.sessions.length} {ws.sessions.length === 1 ? "session" : "sessions"}
            </div>
          </div>
          {ws.counts.needsYou > 0 ? (
            <span className="shrink-0 rounded-full bg-amber-500/15 px-2 py-0.5 text-[11px] text-amber-300">{ws.counts.needsYou} need you</span>
          ) : ws.syncError ? (
            <span className="shrink-0 rounded-full bg-amber-500/15 px-2 py-0.5 text-[11px] text-amber-300">Out of date</span>
          ) : (
            <span className="shrink-0 rounded-full bg-emerald-500/15 px-2 py-0.5 text-[11px] text-emerald-300">{allDone ? "Done" : "On track"}</span>
          )}
        </div>
        <ProgressBar ws={ws} />
        {ws.syncError && (
          <div className="truncate text-xs text-amber-300" title={ws.syncError}>
            Couldn't sync: {ws.syncError}
          </div>
        )}
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-zinc-400">
          <span>{ws.tasks.length ? `${ws.counts.done} of ${ws.tasks.length} done` : "No task list"}</span>
          <span className="inline-flex items-center gap-1"><Bot size={12} aria-hidden /> {ws.counts.agent} agent</span>
          <span className="inline-flex items-center gap-1"><User size={12} aria-hidden /> {ws.counts.human} human</span>
        </div>
        <div className="flex items-center justify-between gap-2">
          <Avatars ws={ws} />
          <span className="text-xs text-zinc-500">{subs ? `Lead + ${subs} sub-agent${subs === 1 ? "" : "s"}` : ws.sessions.length > 1 ? `${ws.sessions.length} leads` : ws.sessions.length ? "Lead" : ""}</span>
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
            <div className="text-xs text-zinc-500">
              {ws.sessions.length === 0 ? "No sessions yet: open it to add some." : `No updates yet. Last active ${ago(Date.now() - ws.lastActivity)}.`}
            </div>
          )}
        </div>
      </button>
      <StarButton ws={ws} className="absolute right-3 top-3" />
    </div>
  );
}

function Detail({ ws, onBack, onOpen }: { ws: Workstream; onBack: () => void; onOpen: (id: string) => void }) {
  const [sel, setSel] = useState<string | null>(ws.tasks.find((t) => t.status === "in_progress")?.key ?? null);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const add = useWsChange((ids: string[]) => api.addToWorkstream(ws.key, ids));
  const remove = useWsChange((id: string) => api.removeFromWorkstream(ws.key, id));
  const rename = useWsChange((title: string) => api.renameWorkstream(ws.key, title));
  const del = useWsChange(() => api.deleteWorkstream(ws.key));
  const sync = useWsChange(() => api.syncWorkstream(ws.key));
  const items = trackerTasks(ws);
  const syncNow = sync.mutate;
  // Imported ones refresh from the tracker when opened, if it's been a while.
  useEffect(() => {
    if (ws.sourceUrl && Date.now() - (ws.syncedAt ?? 0) > SYNC_AFTER_MS) syncNow();
  }, [ws.key, ws.sourceUrl, ws.syncedAt, syncNow]);
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
          {editing ? (
            <form
              className="flex items-center gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                const title = String(new FormData(e.currentTarget).get("title") ?? "");
                rename.mutate(title, { onSuccess: () => setEditing(false) });
              }}
            >
              <input name="title" autoFocus defaultValue={ws.title} aria-label="Workstream name" className={cx(inputClass, "max-w-md")} onKeyDown={(e) => e.key === "Escape" && setEditing(false)} />
              <Button size="sm" variant="primary" type="submit" busy={rename.isPending}>Save</Button>
              <Button size="sm" variant="ghost" type="button" onClick={() => setEditing(false)}>Cancel</Button>
            </form>
          ) : (
            <div className="flex min-w-0 items-center gap-1">
              <h1 className="truncate text-lg font-semibold text-zinc-50">{ws.title}</h1>
              <StarButton ws={ws} />
              <button className="rounded-md p-1 text-zinc-500 hover:bg-zinc-800 hover:text-zinc-200" aria-label="Rename" title={ws.custom ? "Rename" : "Rename (clear to use the automatic name)"} onClick={() => setEditing(true)}>
                <Pencil size={14} />
              </button>
            </div>
          )}
          <div className="flex flex-wrap items-center gap-x-3 text-xs text-zinc-500">
            {ws.sourceUrl && (
              <button className="inline-flex items-center gap-1 hover:text-zinc-200" onClick={() => api.openTrackerLink(ws.sourceUrl!)}>
                <ExternalLink size={12} aria-hidden /> {trackerName(ws.sourceUrl)} epic
              </button>
            )}
            {ws.sourceUrl && (
              <button
                className="inline-flex items-center gap-1 hover:text-zinc-200 disabled:opacity-60"
                disabled={sync.isPending}
                onClick={() => sync.mutate()}
                title="Fetch the epic and its items again"
              >
                <RefreshCw size={12} aria-hidden className={cx(sync.isPending && "animate-spin")} />
                {sync.isPending ? "Syncing…" : ws.syncedAt ? `Synced ${ago(Date.now() - ws.syncedAt)}` : "Sync"}
              </button>
            )}
            {ws.repo && <span>{projectName(ws.repo)}</span>}
            {ws.branch && <span className="inline-flex items-center gap-1"><GitBranch size={12} aria-hidden /> {ws.branch}</span>}
            <span>{ws.tasks.length ? `${ws.counts.done} of ${ws.tasks.length} done` : "No task list"}</span>
            {ws.counts.needsYou > 0 && <span className="text-amber-300">{ws.counts.needsYou} need you</span>}
          </div>
          <div className="mt-2 max-w-md"><ProgressBar ws={ws} /></div>
          {ws.syncError && !sync.isPending && (
            <p className="mt-2 text-xs text-amber-300">
              Couldn't sync{ws.syncedAt ? `; showing the epic as of ${ago(Date.now() - ws.syncedAt)}` : ""}: {ws.syncError}
            </p>
          )}
        </div>
        <div className="flex shrink-0 flex-col items-end gap-2">
          <Avatars ws={ws} max={8} />
          <div className="flex items-center gap-2">
            <Button size="sm" onClick={() => setAdding(true)}>
              <Plus size={13} /> Add sessions
            </Button>
            {ws.custom &&
              (confirmDelete ? (
                <>
                  <Button size="sm" variant="danger" busy={del.isPending} onClick={() => del.mutate(undefined, { onSuccess: onBack })}>
                    Delete it
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(false)}>Keep</Button>
                </>
              ) : (
                <Button size="sm" variant="ghost" aria-label="Delete workstream" title="Delete workstream (its sessions stay)" onClick={() => setConfirmDelete(true)}>
                  <Trash2 size={14} />
                </Button>
              ))}
          </div>
        </div>
      </div>
      <ErrorNote error={rename.error ?? del.error ?? remove.error ?? sync.error} />
      {adding && (
        <SessionPicker
          heading={`Add sessions to ${ws.title}`}
          action="Add"
          exclude={ws.sessions.map((s) => s.id)}
          busy={add.isPending}
          error={add.error}
          onClose={() => setAdding(false)}
          onDone={(ids) => add.mutate(ids, { onSuccess: () => setAdding(false) })}
        />
      )}

      <div className="grid gap-4 lg:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)]">
        <Card title="Tasks">
          <div className="space-y-4" role="tree" aria-label="Tasks by session">
            {items.length > 0 && (
              <div role="group">
                <div className="mb-1 flex items-center gap-2">
                  <span className="truncate text-sm font-medium text-zinc-200">From {trackerName(ws.sourceUrl ?? "")}</span>
                  <span className="text-xs text-zinc-500">{items.filter((t) => t.status === "done").length} of {items.length} closed</span>
                </div>
                {items.map((t) => (
                  <TaskRow key={t.key} t={t} selected={t.key === sel} onSelect={() => setSel(t.key === sel ? null : t.key)} />
                ))}
              </div>
            )}
            {ws.sessions.length === 0 && (
              <div className="text-sm text-zinc-500">
                No sessions yet.{" "}
                <button className="text-emerald-400 hover:underline" onClick={() => setAdding(true)}>
                  Add sessions
                </button>{" "}
                and their feature branches come along, so later sessions on them join too.
              </div>
            )}
            {tree(ws).map(({ session, tasks }) => (
              <div key={session.id} role="group">
                <div className="mb-1 flex items-center gap-2">
                  <span className="truncate text-sm font-medium text-zinc-200">{session.title ?? "Untitled session"}</span>
                  <StatusBadge status={session.status} size="xs" />
                  <SourceBadge source={session.source} entrypoint={session.entrypoint} />
                  <button className="ml-auto shrink-0 text-xs text-zinc-500 hover:text-zinc-200" onClick={() => onOpen(session.id)}>
                    Open
                  </button>
                  {ws.custom && (
                    <button
                      className="shrink-0 text-xs text-zinc-500 hover:text-rose-300"
                      title="Take it, and the branch it brought, out of this workstream"
                      onClick={() => remove.mutate(session.id)}
                    >
                      Remove
                    </button>
                  )}
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
      <div className="flex items-start justify-between gap-2">
        <div className="text-sm font-medium text-zinc-100">{t.title}</div>
        {t.url && (
          <Button size="sm" variant="ghost" onClick={() => api.openTrackerLink(t.url!)}>
            <ExternalLink size={13} /> Open
          </Button>
        )}
      </div>
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
            <dt className="text-zinc-500">{t.sessionId ? "Worked on by" : "Assigned to"}</dt>
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

/**
 * Pick recent sessions to put in a workstream, by searching their titles and
 * prompts; with `named`, it also asks for the new workstream's name.
 */
function SessionPicker({
  heading,
  named,
  action,
  exclude = [],
  busy,
  error,
  onClose,
  onDone,
}: {
  heading: string;
  named?: boolean;
  action: string;
  exclude?: string[];
  busy: boolean;
  error: unknown;
  onClose: () => void;
  onDone: (ids: string[], title: string) => void;
}) {
  const [title, setTitle] = useState("");
  const [search, setSearch] = useState("");
  const [picked, setPicked] = useState<string[]>([]);
  const q = useQuery({ queryKey: ["sessions", "picker", search], queryFn: () => api.sessions({ days: 90, search: search || undefined }) });
  const rows = (q.data ?? []).filter((s) => !exclude.includes(s.id)).slice(0, 60);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !busy && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose]);
  const toggle = (id: string) => setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id]));
  const ready = named ? title.trim().length > 0 : picked.length > 0;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-6" role="dialog" aria-modal="true" aria-labelledby="picker-title">
      <button aria-label="Close" className="absolute inset-0 bg-black/50" onClick={() => !busy && onClose()} />
      <form
        className="relative flex max-h-full w-full max-w-xl flex-col overflow-hidden rounded-2xl border border-zinc-800 bg-zinc-950 shadow-2xl"
        onSubmit={(e) => {
          e.preventDefault();
          if (ready) onDone(picked, title);
        }}
      >
        <div className="flex items-start justify-between gap-3 border-b border-zinc-800 px-5 py-4">
          <div className="min-w-0">
            <h2 id="picker-title" className="truncate text-base font-semibold text-zinc-50">{heading}</h2>
            <p className="text-xs text-zinc-500">A session brings its feature branch along, so later sessions on that branch join too.</p>
          </div>
          <Button variant="ghost" size="sm" type="button" onClick={onClose} disabled={busy} aria-label="Close">
            <X size={16} />
          </Button>
        </div>
        <div className="space-y-3 border-b border-zinc-800 px-5 py-4">
          {named && <input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Name, e.g. Checkout revamp" aria-label="Name" className={inputClass} />}
          <input autoFocus={!named} value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search sessions from the last 90 days" aria-label="Search sessions" className={inputClass} />
        </div>
        <ul className="min-h-0 flex-1 divide-y divide-zinc-800/70 overflow-y-auto px-2 py-1" aria-label="Sessions">
          {q.isLoading && <li className="px-3 py-3 text-sm text-zinc-500">Loading…</li>}
          {!q.isLoading && rows.length === 0 && <li className="px-3 py-3 text-sm text-zinc-500">No sessions match.</li>}
          {rows.map((s) => (
            <li key={s.id}>
              <label className="flex cursor-pointer items-center gap-3 rounded-md px-3 py-2 hover:bg-zinc-900">
                <input type="checkbox" checked={picked.includes(s.id)} onChange={() => toggle(s.id)} className="accent-emerald-500" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm text-zinc-200">{s.title ?? s.firstPrompt ?? "Untitled session"}</span>
                  <span className="block truncate text-xs text-zinc-500">
                    {[s.repoRoot ?? s.projectPath ? projectName((s.repoRoot ?? s.projectPath)!) : null, s.gitBranch, s.endedAt ? ago(Date.now() - s.endedAt) : null]
                      .filter(Boolean)
                      .join(" · ")}
                  </span>
                </span>
                <SourceBadge source={s.source} entrypoint={s.entrypoint} />
              </label>
            </li>
          ))}
        </ul>
        <div className="flex items-center justify-between gap-3 border-t border-zinc-800 px-5 py-3">
          <span className="text-xs text-zinc-500">{picked.length ? `${picked.length} selected` : named ? "You can add sessions later" : "Pick sessions"}</span>
          <ErrorNote error={error} />
          <Button variant="primary" size="sm" type="submit" disabled={!ready} busy={busy}>
            {action}
          </Button>
        </div>
      </form>
    </div>
  );
}

/** Import a workstream from a GitHub issue or Azure DevOps work item link. */
function ImportDialog({ onClose, onDone }: { onClose: () => void; onDone: (key: string) => void }) {
  const [url, setUrl] = useState("");
  const imp = useWsChange((link: string) => api.importWorkstream(link));
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !imp.isPending && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [imp.isPending, onClose]);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-6" role="dialog" aria-modal="true" aria-labelledby="import-title">
      <button aria-label="Close" className="absolute inset-0 bg-black/50" onClick={() => !imp.isPending && onClose()} />
      <form
        className="relative w-full max-w-lg rounded-2xl border border-zinc-800 bg-zinc-950 p-5 shadow-2xl"
        onSubmit={(e) => {
          e.preventDefault();
          if (url.trim()) imp.mutate(url.trim(), { onSuccess: (key) => onDone(key as string) });
        }}
      >
        <h2 id="import-title" className="text-base font-semibold text-zinc-50">Import an epic</h2>
        <p className="mt-1 text-xs text-zinc-500">
          A GitHub issue (its sub-issues or task list become tasks) or an Azure DevOps epic or feature (its child items). Sessions that mention it or its items join. Tilly uses your
          own <code>gh</code> or <code>az</code> sign-in and stores no token.
        </p>
        <input
          autoFocus
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://github.com/owner/repo/issues/12 or https://dev.azure.com/org/project/_workitems/edit/34"
          aria-label="Issue or work item link"
          className={cx(inputClass, "mt-4")}
        />
        <ErrorNote error={imp.error} />
        <div className="mt-4 flex justify-end gap-2">
          <Button type="button" variant="ghost" size="sm" onClick={onClose} disabled={imp.isPending}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" size="sm" disabled={!url.trim()} busy={imp.isPending}>
            Import
          </Button>
        </div>
      </form>
    </div>
  );
}
