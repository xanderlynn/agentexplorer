import { memo, useMemo, useState, useSyncExternalStore, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { Area, AreaChart, Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { ArrowRight, ChevronDown, Radio, Star, TriangleAlert, X } from "lucide-react";
import { api, useLive, type LiveBucket, type LiveSession, type LiveSummary, type Source } from "../api";
import { StatusAvatar, StatusBadge, TeamStack } from "../components/status";
import { Swimlanes } from "../components/Swimlanes";
import { Card, Empty, ErrorNote, Meter, PageHeader, Segmented, SourceBadge, Stat, cx } from "../components/ui";
import { MODE_LABEL, SMART_ZONE_DEFAULT, SOURCE_LABEL, SOURCE_OPTIONS, compact, duration, projectName } from "../format";
import { MIN_N } from "../stats";
import { STATUS, isActive, isAwaiting, urgency } from "../status";
import { chart } from "../theme";

type ToolFilter = Source | "all";
/** A KPI tile narrows the board to its part. */
type Focus = "awaiting" | "working" | "completed" | "agents" | null;
const FOCUS_LABEL: Record<Exclude<Focus, null>, string> = { awaiting: "Awaiting you", working: "Working", completed: "Completed today", agents: "Sessions with sub-agents" };

// One clock for every timer on the page. Only the small labels that show a
// running time subscribe to it, so the page itself re-renders once per poll,
// not once a second.
let clockNow = Date.now();
const clockSubs = new Set<() => void>();
let clockTimer: ReturnType<typeof setInterval> | undefined;
function subscribeClock(fn: () => void) {
  clockSubs.add(fn);
  clockTimer ??= setInterval(() => {
    clockNow = Date.now();
    clockSubs.forEach((f) => f());
  }, 1000);
  return () => {
    clockSubs.delete(fn);
    if (clockSubs.size === 0) {
      clearInterval(clockTimer);
      clockTimer = undefined;
    }
  };
}
const useClock = () => useSyncExternalStore(subscribeClock, () => clockNow);

/** Time since `from`, ticking every second. */
function Since({ from }: { from: number }) {
  return <>{duration(Math.max(0, useClock() - from))}</>;
}

/** Time until `to`, ticking every second. */
function Until({ to }: { to: number }) {
  return <>{duration(Math.max(0, to - useClock()))}</>;
}

/**
 * The live control center: what needs you, what is running, what finished
 * today, and how today's work between you and your agents is going.
 */
export default function ControlCenter({ onOpen }: { onOpen: (id: string) => void }) {
  const q = useLive();
  const [tool, setTool] = useState<ToolFilter>("all");
  const [teamsOnly, setTeamsOnly] = useState(false);
  const [focus, setFocus] = useState<Focus>(null);
  // Context past this is flagged on each card.
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const zone = Number(settings.data?.values["context.smartZoneTokens"]) || SMART_ZONE_DEFAULT;

  const header = (
    <PageHeader
      title="Control center"
      subtitle="Every agent session: what needs you, what is running, and what finished today."
      actions={
        <div className="flex flex-wrap items-center gap-2">
          <Segmented
            value={tool}
            onChange={setTool}
            options={SOURCE_OPTIONS}
          />
          <label className="flex items-center gap-1.5 text-xs text-zinc-400">
            <input type="checkbox" className="accent-emerald-500" checked={teamsOnly} onChange={(e) => setTeamsOnly(e.target.checked)} />
            Teams only
          </label>
          <span className="inline-flex items-center gap-1 text-xs text-zinc-500" title="Refreshes every 5 seconds">
            <Radio size={12} className={cx("text-zinc-400 transition-opacity", q.isFetching ? "opacity-100" : "opacity-50")} aria-hidden /> Live
          </span>
        </div>
      }
    />
  );
  if (q.isLoading) return <>{header}<Skeleton /></>;
  if (q.error) return <>{header}<ErrorNote error={q.error} /></>;
  const d = q.data!;
  const keep = (s: LiveSession) => (tool === "all" || s.source === tool) && (!(teamsOnly || focus === "agents") || s.agents.length > 0);
  const show = (part: Exclude<Focus, null>) => focus == null || focus === part || focus === "agents";
  const sessions = d.sessions.filter(keep);
  const awaiting = sessions
    .filter((s) => isAwaiting(s.status))
    .sort((a, b) => urgency(a.status) - urgency(b.status) || a.statusSince - b.statusSince);
  const working = sessions.filter((s) => isActive(s.status)).sort((a, b) => urgency(a.status) - urgency(b.status) || b.endedAt - a.endedAt);
  const scheduled = sessions.filter((s) => s.status === "scheduled");
  const idle = sessions.filter((s) => s.status === "stale" || s.status === "done" || s.status === "ended");
  const completed = d.completedToday.filter(keep);

  return (
    <>
      {header}
      <div className="space-y-6" onKeyDown={moveFocus}>
        <Kpis d={d} focus={focus} setFocus={setFocus} />
        {focus && (
          <div className="flex items-center gap-2 text-xs text-zinc-400">
            Showing
            <button onClick={() => setFocus(null)} className="inline-flex items-center gap-1 rounded-md bg-zinc-800 px-2 py-0.5 text-zinc-200 hover:bg-zinc-700">
              {FOCUS_LABEL[focus]} <X size={12} aria-label="Show everything" />
            </button>
          </div>
        )}

        {show("awaiting") && <section aria-labelledby="awaiting">
          <h2 id="awaiting" className="mb-2 flex items-center gap-2 text-sm font-medium text-zinc-200">
            Awaiting you <span className="tabular-nums text-zinc-500">{awaiting.length}</span>
            <span className="text-xs font-normal text-zinc-500">· longest wait first</span>
          </h2>
          {awaiting.length === 0 ? (
            <div className="rounded-xl border border-dashed border-zinc-800 px-4 py-3 text-sm text-zinc-500">Nothing is waiting on you.</div>
          ) : (
            <ul className="space-y-2">
              {awaiting.map((s) => (
                <AwaitingRow key={s.id} s={s} onOpen={onOpen} />
              ))}
            </ul>
          )}
        </section>}

        <div className={cx("grid gap-4", focus === "working" || focus === "completed" ? "" : "lg:grid-cols-3")}>
          {show("working") && <Column title="Working" count={working.length} empty="No agent is working right now.">
            {working.map((s) => (
              <SessionCard key={s.id} s={s} zone={zone} onOpen={onOpen} />
            ))}
            {idle.length > 0 && <IdleFooter sessions={idle} onOpen={onOpen} />}
          </Column>}
          {focus == null && <Column title="Scheduled" count={scheduled.length} empty="Nothing scheduled. Jobs an agent sets up with a cron or wake-up show here.">
            {scheduled.map((s) => (
              <SessionCard key={s.id} s={s} zone={zone} onOpen={onOpen} />
            ))}
          </Column>}
          {show("completed") && <Column title="Completed today" count={completed.length} empty="Nothing finished yet today.">
            {completed.slice(0, 12).map((s) => (
              <CompletedRow key={s.id} s={s} onOpen={onOpen} />
            ))}
            {completed.length > 12 && <div className="px-1 text-xs text-zinc-500">+{completed.length - 12} more in Sessions</div>}
          </Column>}
        </div>

        <TodayCharts buckets={d.buckets} replyHistogram={d.replyHistogram} replyN={d.replyN} midnight={d.midnight} onOpen={onOpen} />
      </div>
    </>
  );
}

/** j / k (or arrow keys) move between cards; Enter opens one. */
function moveFocus(e: React.KeyboardEvent<HTMLDivElement>) {
  if ((e.target as HTMLElement).closest("input, select, textarea")) return;
  const down = e.key === "j" || e.key === "ArrowDown";
  const up = e.key === "k" || e.key === "ArrowUp";
  if (!down && !up) return;
  // Only cards on screen: not those inside a closed section.
  const cards = [...e.currentTarget.querySelectorAll<HTMLElement>("[data-card]")].filter((el) => el.offsetParent !== null);
  const i = cards.indexOf(document.activeElement as HTMLElement);
  // Arrow keys move only between cards; j / k can start from anywhere.
  if (i < 0 && (e.key === "ArrowDown" || e.key === "ArrowUp")) return;
  const next = cards[Math.max(0, Math.min(cards.length - 1, i + (down ? 1 : -1)))];
  if (next) {
    e.preventDefault();
    next.focus();
  }
}

function Kpis({ d, focus, setFocus }: { d: LiveSummary; focus: Focus; setFocus: (f: Focus) => void }) {
  const pick = (f: Exclude<Focus, null>) => () => setFocus(focus === f ? null : f);
  // Only the buckets up to now, last 3 hours, for sparklines.
  const recent = d.buckets.slice(-12);
  // The earliest a waiting session started waiting: the longest wait.
  const oldest = d.sessions.filter((s) => isAwaiting(s.status)).reduce((m, s) => Math.min(m, s.statusSince), Infinity);
  const share = d.agentMsToday + d.waitMsToday > 0 ? d.waitMsToday / (d.agentMsToday + d.waitMsToday) : null;
  return (
    <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
      <Stat label="Working" value={d.working} spark={recent.map((b) => b.working)} hint={`busiest 15 min today: ${d.peakParallelToday}`} onClick={pick("working")} />
      <Stat label="Awaiting you" value={d.awaiting} spark={recent.map((b) => b.awaiting)} hint={Number.isFinite(oldest) ? <>longest <Since from={oldest} /></> : "nothing waiting"} onClick={pick("awaiting")} />
      <Stat label="Completed today" value={d.completedToday.length} spark={recent.map((b) => b.done)} hint={`${d.turnsToday} agent turns`} onClick={pick("completed")} />
      <Stat
        label="Sub-agents"
        onClick={pick("agents")}
        value={d.agentsRunning}
        hint={
          <>
            running · {d.agentsDoneToday} done today
            {d.agentsFailedToday > 0 && <span className="text-rose-300"> · {d.agentsFailedToday} failed</span>}
          </>
        }
      />
      <Stat label="Waited on you" value={duration(d.waitMsToday)} hint={share != null ? `${Math.round(share * 100)}% of today's agent time` : "today"} />
      <Stat
        label="Your reply time"
        value={d.replyN >= MIN_N && d.replyMedianMs != null ? duration(d.replyMedianMs) : "—"}
        n={d.replyN || undefined}
        hint={d.replyN >= MIN_N && d.replyP75Ms != null ? `median · 75% within ${duration(d.replyP75Ms)}` : `needs ${MIN_N} replies today`}
      />
    </div>
  );
}

// Rows, cards and charts skip re-rendering when a poll brings the same data:
// react-query keeps unchanged parts of the response as the same objects.
const AwaitingRow = memo(function AwaitingRow({ s, onOpen }: { s: LiveSession; onOpen: (id: string) => void }) {
  const t = STATUS[s.status];
  const note = s.status === "maybe_blocked" && s.pendingTool ? `Waiting on ${s.pendingTool} — check the terminal for a permission prompt.` : s.lastMessage;
  return (
    <li>
      <button
        data-card
        onClick={() => onOpen(s.id)}
        className={cx("flex w-full items-start gap-3 rounded-xl border bg-zinc-900/60 p-3 text-left ring-1 ring-inset transition-colors hover:bg-zinc-800/50 focus-visible:outline-2 focus-visible:outline-emerald-500", "border-zinc-800", t.ring)}
      >
        <StatusAvatar status={s.status} size={44} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium text-zinc-100">{s.title ?? "Untitled session"}</div>
          <div className="mt-1 flex flex-wrap items-center gap-2">
            <StatusBadge status={s.status} />
            <span className={cx("text-sm font-semibold tabular-nums", t.text)}>
              <Since from={s.statusSince} />
            </span>
          </div>
          <div className="mt-0.5 flex flex-wrap items-center gap-x-2 text-xs text-zinc-500">
            <SourceBadge source={s.source} />
            <span>{projectName(s.projectPath)}</span>
            {s.gitBranch && <span>· {s.gitBranch}</span>}
            {s.agents.length > 0 && <TeamStack agents={s.agents} />}
          </div>
          {note && <div className="mt-1.5 line-clamp-2 border-l-2 border-zinc-700 pl-2 text-xs text-zinc-400">{note}</div>}
        </div>
        <span className="mt-1 inline-flex shrink-0 items-center gap-1 text-xs text-zinc-400">
          Open <ArrowRight size={13} aria-hidden />
        </span>
      </button>
    </li>
  );
});

function Column({ title, count, empty, children }: { title: string; count: number; empty: string; children: ReactNode }) {
  return (
    <section className="rounded-xl border border-zinc-800/80 bg-zinc-900/30 p-3" aria-label={title}>
      <h2 className="mb-2 flex items-center justify-between px-1 text-sm font-medium text-zinc-200">
        {title} <span className="tabular-nums text-zinc-500">{count}</span>
      </h2>
      <div className="space-y-2">{count === 0 && <p className="px-1 py-2 text-xs text-zinc-500">{empty}</p>}{children}</div>
    </section>
  );
}

const SessionCard = memo(function SessionCard({ s, zone, onOpen }: { s: LiveSession; zone: number; onOpen: (id: string) => void }) {
  const running = s.agents.filter((a) => a.status === "running" || a.status === "quiet").length;
  return (
    <button data-card onClick={() => onOpen(s.id)} className="block w-full rounded-lg border border-zinc-800 bg-zinc-900/70 p-2.5 text-left hover:border-zinc-700 focus-visible:outline-2 focus-visible:outline-emerald-500">
      <div className="flex items-center justify-between gap-2">
        <StatusBadge status={s.status} size="xs" />
        <span className="text-xs tabular-nums text-zinc-500">
          {s.status === "scheduled" && s.nextRun ? (
            <>
              next in <Until to={s.nextRun} />
            </>
          ) : (
            <Since from={s.statusSince} />
          )}
        </span>
      </div>
      <div className="mt-1.5 truncate text-sm text-zinc-100">{s.title ?? "Untitled session"}</div>
      <div className="mt-0.5 flex min-w-0 items-center gap-2 text-xs text-zinc-500">
        <span className="shrink-0">{SOURCE_LABEL[s.source]}</span>
        <span className="min-w-0 truncate">· {projectName(s.projectPath)}</span>
        {s.mode && isActive(s.status) && <span>· {MODE_LABEL[s.mode].toLowerCase()}</span>}
        {s.pendingTool && isActive(s.status) && <span className="truncate font-mono">· {s.pendingTool}</span>}
      </div>
      {s.agents.length > 0 && (
        <div className="mt-2 flex items-center justify-between gap-2">
          <TeamStack agents={s.agents} />
          <span className="text-xs text-zinc-500">{running ? `${running} running` : `${s.agents.length} agents`}</span>
        </div>
      )}
      {s.context > 0 && (
        <div className="mt-2 flex items-center gap-2" title={`${compact(s.context)} tokens of context; your smart zone is ${compact(zone)}`}>
          <Meter value={s.context / zone} color="var(--color-zinc-500)" />
          <span className="inline-flex shrink-0 items-center gap-1 text-[11px] tabular-nums text-zinc-400">
            {s.context > zone && <TriangleAlert size={11} aria-label="Over your smart zone" />}
            {compact(s.context)}
          </span>
        </div>
      )}
    </button>
  );
});

function IdleFooter({ sessions, onOpen }: { sessions: LiveSession[]; onOpen: (id: string) => void }) {
  return (
    <details className="group">
      <summary className="flex cursor-pointer list-none items-center gap-1 px-1 text-xs text-zinc-500 hover:text-zinc-300">
        <ChevronDown size={13} className="transition-transform group-open:rotate-180" aria-hidden /> Idle or finished earlier ({sessions.length})
      </summary>
      <ul className="mt-1 space-y-0.5">
        {sessions.map((s) => (
          <li key={s.id}>
            <button data-card onClick={() => onOpen(s.id)} className="flex w-full items-center gap-2 rounded px-1 py-1 text-left text-xs hover:bg-zinc-800/40 focus-visible:outline-2 focus-visible:outline-emerald-500">
              <StatusBadge status={s.status} size="xs" label={false} />
              <span className="truncate text-zinc-300">{s.title ?? "Untitled session"}</span>
            </button>
          </li>
        ))}
      </ul>
    </details>
  );
}

const CompletedRow = memo(function CompletedRow({ s, onOpen }: { s: LiveSession; onOpen: (id: string) => void }) {
  const failed = s.agents.filter((a) => a.status === "failed" || a.status === "stopped").length;
  return (
    <button data-card onClick={() => onOpen(s.id)} className="flex w-full items-center gap-2 rounded-lg px-1.5 py-1.5 text-left hover:bg-zinc-800/40 focus-visible:outline-2 focus-visible:outline-emerald-500">
      <StatusBadge status={s.status} size="xs" label={false} />
      <span className="min-w-0 flex-1 truncate text-sm text-zinc-200">{s.title ?? "Untitled session"}</span>
      {failed > 0 && <span className="shrink-0 text-xs text-rose-300">{failed} agent{failed > 1 ? "s" : ""} failed</span>}
      <span className="shrink-0 text-xs tabular-nums text-zinc-500">{duration(s.activeMs)}</span>
      <span className="inline-flex w-9 shrink-0 items-center justify-end gap-0.5 text-xs tabular-nums text-zinc-400" title={s.rating ? `You rated it ${s.rating} of 5` : "Not rated yet"}>
        {s.rating ? (
          <>
            <Star size={11} aria-hidden /> {s.rating}
          </>
        ) : (
          "—"
        )}
      </span>
    </button>
  );
});

const HIST = ["<1m", "1–5m", "5–15m", "15–60m", ">1h"];
const time = (t: number) => new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

const TodayCharts = memo(function TodayCharts({
  buckets,
  replyHistogram,
  replyN,
  midnight,
  onOpen,
}: {
  buckets: LiveBucket[];
  replyHistogram: LiveSummary["replyHistogram"];
  replyN: number;
  midnight: number;
  onOpen: (id: string) => void;
}) {
  // The day's lanes change slowly; once a minute is plenty.
  const lanes = useQuery({ queryKey: ["timelines", midnight], queryFn: () => api.timelines(midnight), refetchInterval: 60_000 });
  const series = useMemo(() => buckets.map((b) => ({ t: time(b.start), working: b.working, awaiting: b.awaiting })), [buckets]);
  const hist = useMemo(() => replyHistogram.map((n, i) => ({ bucket: HIST[i], n })), [replyHistogram]);
  return (
    <div className="space-y-4">
      <Card title="Today's activity">
        {lanes.data && lanes.data.length > 0 ? (
          <Swimlanes lanes={lanes.data} since={midnight} onOpen={onOpen} />
        ) : (
          <Empty title="No activity yet today" />
        )}
      </Card>
      <div className="grid gap-4 lg:grid-cols-2">
        <Card title="Sessions working and awaiting you, through the day">
          <div className="h-44">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={series} margin={{ left: -24, right: 4, top: 6 }}>
                <CartesianGrid stroke={chart.grid} vertical={false} />
                <XAxis dataKey="t" {...chart.axisProps} minTickGap={40} />
                <YAxis allowDecimals={false} {...chart.axisProps} />
                <Tooltip {...chart.tooltip} />
                <Area type="stepAfter" dataKey="working" name="Working" stroke="var(--color-green-400)" fill="var(--color-green-500)" fillOpacity={0.2} isAnimationActive={false} />
                <Area type="stepAfter" dataKey="awaiting" name="Awaiting you" stroke="var(--color-amber-400)" fill="var(--color-amber-500)" fillOpacity={0.15} strokeDasharray="4 2" isAnimationActive={false} />
              </AreaChart>
            </ResponsiveContainer>
          </div>
          <p className="mt-1 text-xs text-zinc-500">Solid green: working. Dashed amber: awaiting you. In 15-minute steps.</p>
        </Card>
        <Card title={`How long you took to reply today (${replyN})`}>
          {replyN === 0 ? (
            <Empty title="No replies yet today" />
          ) : (
            <div className="h-44">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={hist} margin={{ left: -24, right: 4, top: 6 }}>
                  <CartesianGrid stroke={chart.grid} vertical={false} />
                  <XAxis dataKey="bucket" {...chart.axisProps} />
                  <YAxis allowDecimals={false} {...chart.axisProps} />
                  <Tooltip {...chart.tooltip} formatter={(v) => [v, "replies"]} />
                  <Bar dataKey="n" fill="var(--color-amber-500)" radius={[3, 3, 0, 0]} isAnimationActive={false} />
                </BarChart>
              </ResponsiveContainer>
            </div>
          )}
        </Card>
      </div>
    </div>
  );
});

function Skeleton() {
  return (
    <div className="space-y-6" aria-busy="true" aria-label="Loading">
      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
        {Array.from({ length: 6 }, (_, i) => (
          <div key={i} className="h-20 rounded-xl bg-zinc-900/60 motion-safe:animate-pulse" />
        ))}
      </div>
      <div className="h-24 rounded-xl bg-zinc-900/60 motion-safe:animate-pulse" />
      <div className="grid gap-4 lg:grid-cols-3">
        {Array.from({ length: 3 }, (_, i) => (
          <div key={i} className="h-48 rounded-xl bg-zinc-900/40 motion-safe:animate-pulse" />
        ))}
      </div>
    </div>
  );
}
