import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowRight, ClipboardList, Flag, FlaskConical, Target } from "lucide-react";
import { api, useLive } from "../api";
import type { Drill, Page } from "../App";
import { QuickReview } from "../components/QuickReview";
import { FirstRun } from "../components/FirstRun";
import { StatusBadge } from "../components/status";
import { STATUS, isActive, isAwaiting, mostUrgent } from "../status";
import { Button, Card, Loading, PageHeader, SourceBadge, cx } from "../components/ui";
import { dateTime, duration, projectName } from "../format";
import { fmtMetric } from "../metrics";
import { InsightItem } from "../components/InsightItem";

/**
 * Planning and review: a pointer to anything awaiting you, recent sessions
 * to rate in a tap, the experiment you're running, your goals, and your
 * planned tasks.
 * Decisions, not volume: there are no activity totals here.
 */
export default function Today({
  onOpen,
  go,
  drill,
  plan,
}: {
  onOpen: (id: string) => void;
  go: (p: Page) => void;
  drill: (d: Drill) => void;
  plan: () => void;
}) {
  const live = useLive();
  const toRate = useQuery({ queryKey: ["sessions", { days: 3, outcome: "unrated" }], queryFn: () => api.sessions({ days: 3, outcome: "unrated" }) });
  const exps = useQuery({ queryKey: ["experiments"], queryFn: api.experiments });
  const goals = useQuery({ queryKey: ["goals"], queryFn: api.goals });
  const intents = useQuery({ queryKey: ["intents"], queryFn: api.intents });
  const insights = useQuery({ queryKey: ["insights", 30], queryFn: () => api.insights(30) });
  const unusual = useQuery({ queryKey: ["anomalies", 14], queryFn: () => api.anomalies(14) });

  const away = useAway();
  const waiting = live.data?.sessions.filter((s) => isAwaiting(s.status)) ?? [];
  // Not finished yet: nothing to rate while it's working or waiting on you.
  const unfinished = new Set(live.data?.sessions.filter((s) => isAwaiting(s.status) || isActive(s.status)).map((s) => s.id));
  const rateable = toRate.data?.filter((s) => !unfinished.has(s.id));
  const active = exps.data?.filter((e) => e.status === "active") ?? [];
  const open = intents.data?.filter((i) => i.closedAt == null) ?? [];
  const now = new Date();
  const greeting = now.getHours() < 12 ? "Good morning" : now.getHours() < 18 ? "Good afternoon" : "Good evening";

  return (
    <>
      <PageHeader
        title={greeting}
        subtitle={now.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" })}
        actions={
          <Button variant="primary" onClick={plan}>
            <ClipboardList size={15} /> Plan a task
          </Button>
        }
      />

      <FirstRun />
      {away && <AwayDigest since={away} onOpen={onOpen} />}

      {waiting.length > 0 && (
        <button
          onClick={() => go("control")}
          className={cx("mb-4 flex w-full items-center gap-3 rounded-xl px-4 py-2.5 text-left text-sm ring-1 ring-inset", STATUS.asking.bg, STATUS.asking.ring)}
        >
          <StatusBadge status={mostUrgent(waiting.map((s) => s.status)) ?? "asking"} />
          <span className="flex-1 text-zinc-100">
            {waiting.length} agent{waiting.length === 1 ? "" : "s"} awaiting you · longest {duration(Math.max(...waiting.map((s) => Date.now() - s.statusSince)))}
          </span>
          <span className="inline-flex items-center gap-1 text-xs text-zinc-300">
            Control center <ArrowRight size={13} aria-hidden />
          </span>
        </button>
      )}

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
        <Card title={`Rate recent sessions (${rateable?.length ?? 0})`} className="xl:col-span-2" action={toRate.data?.length ? <button className="text-xs text-zinc-400 hover:text-zinc-200" onClick={() => drill({ label: "unrated sessions", outcome: "unrated" })}>All unrated →</button> : undefined}>
          {toRate.isLoading && <Loading />}
          {rateable?.length === 0 && <p className="text-sm text-zinc-500">All caught up.</p>}
          <ul className="divide-y divide-zinc-800/70">
            {rateable?.slice(0, 6).map((s) => (
              <li key={s.id} className="py-3">
                <button className="mb-2 block w-full text-left" onClick={() => onOpen(s.id)}>
                  <span className="block truncate text-sm text-zinc-100">{s.title ?? "Untitled"}</span>
                  <span className="flex items-center gap-2 text-xs text-zinc-500">
                    <SourceBadge source={s.source} entrypoint={s.entrypoint} /> · {projectName(s.repoRoot ?? s.projectPath)} · {duration(s.activeMs)} · {dateTime(s.startedAt)}
                    {s.commits > 0 && ` · ${s.commits} commits`}
                  </span>
                </button>
                <QuickReview id={s.id} compact suggested={s.suggestedOutcome} evidence={s.evidence} />
              </li>
            ))}
          </ul>
        </Card>

        <div className="space-y-4">
          <Card title="Running experiments" action={<button className="text-xs text-zinc-400 hover:text-zinc-200" onClick={() => go("experiments")}>All →</button>}>
            {active.length === 0 ? (
              <p className="text-sm text-zinc-500">
                None running.{" "}
                <button className="text-emerald-400 hover:underline" onClick={() => go("experiments")}>
                  Start one
                </button>{" "}
                or adopt one from your retro.
              </p>
            ) : (
              <ul className="space-y-2">
                {active.map((e) => (
                  <li key={e.id} className="flex gap-2 text-sm">
                    <FlaskConical size={15} className="mt-0.5 shrink-0 text-sky-400" aria-hidden />
                    <span>
                      <span className="text-zinc-100">{e.title}</span>
                      <span className="block text-xs text-zinc-500">
                        {e.metric.label}: {fmtMetric(e.metric, e.baselineValue)} → {fmtMetric(e.metric, e.currentValue)} (n={e.currentN}) · ends {new Date(e.endAt).toLocaleDateString()}
                      </span>
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </Card>

          <Card title="Goals this week">
            {goals.data?.length === 0 && (
              <p className="text-sm text-zinc-500">
                No goals.{" "}
                <button className="text-emerald-400 hover:underline" onClick={() => go("experiments")}>
                  Set up to three
                </button>
                .
              </p>
            )}
            <ul className="space-y-2">
              {goals.data?.map((g) => (
                <li key={g.id} className="flex gap-2 text-sm">
                  <Target size={15} className={cx("mt-0.5 shrink-0", g.met ? "text-emerald-400" : "text-amber-400")} aria-hidden />
                  <span>
                    <span className="text-zinc-100">
                      {g.metric.label}: {fmtMetric(g.metric, g.current)}
                    </span>
                    <span className="block text-xs text-zinc-500">
                      target {g.metric.higherIsBetter ? "≥" : "≤"} {fmtMetric(g.metric, g.target)} · {g.met == null ? "no data" : g.met ? "on target" : "not yet"}
                      {g.counterSlipping && <span className="text-rose-300"> · guard slipping</span>}
                    </span>
                  </span>
                </li>
              ))}
            </ul>
          </Card>

          <Card title={`Planned tasks (${open.length})`} action={<button className="text-xs text-zinc-400 hover:text-zinc-200" onClick={() => go("journal")}>Journal →</button>}>
            {open.length === 0 ? (
              <p className="text-sm text-zinc-500">Nothing planned. Plan before you delegate: it doubles as your opening prompt.</p>
            ) : (
              <ul className="space-y-1.5">
                {open.slice(0, 5).map((i) => (
                  <li key={i.id} className="text-sm">
                    <span className="text-zinc-200">{i.goal}</span>
                    <span className="block text-xs text-zinc-500">
                      {i.taskType} · {i.expectedMinutes} min · {i.sessionId ? "session linked" : "waiting for a session"}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </Card>
        </div>
      </div>

      {unusual.data && unusual.data.length > 0 && (
        <Card title="Unusual sessions (last 2 weeks)" className="mt-4">
          <ul className="divide-y divide-zinc-800/70">
            {unusual.data.slice(0, 5).map((a) => (
              <li key={a.id}>
                <button className="flex w-full items-start gap-3 py-2 text-left hover:bg-zinc-800/30" onClick={() => onOpen(a.id)}>
                  <Flag size={15} className="mt-0.5 shrink-0 text-amber-400" aria-label="Unusual" />
                  <span className="min-w-0">
                    <span className="block truncate text-sm text-zinc-100">{a.title ?? "Untitled"}</span>
                    <span className="block text-xs text-zinc-400">{a.reasons.join("; ")}</span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </Card>
      )}

      {insights.data && insights.data.length > 0 && (
        <Card title="Worth a look" className="mt-4" action={<button className="text-xs text-zinc-400 hover:text-zinc-200" onClick={() => go("overview")}>All insights →</button>}>
          <ul className="space-y-1">
            {insights.data
              .filter((i) => i.kind !== "info" || i.sessionIds.length > 0)
              .slice(0, 3)
              .map((i) => (
                <InsightItem key={i.id} insight={i} onOpen={i.sessionIds.length ? () => drill({ label: i.title, ids: i.sessionIds }) : undefined} />
              ))}
          </ul>
        </Card>
      )}
    </>
  );
}

/** Away at least this long, Today says what happened meanwhile. */
const AWAY_MS = 30 * 60_000;

/**
 * When you last had Tilly in front of you, if it was a while ago: kept in
 * local storage and updated whenever the window loses focus or hides.
 */
function useAway() {
  const [away] = useState<number | null>(() => {
    try {
      const last = Number(localStorage.getItem("lastSeen"));
      return last && Date.now() - last > AWAY_MS ? last : null;
    } catch {
      return null;
    }
  });
  useEffect(() => {
    const mark = () => {
      try {
        localStorage.setItem("lastSeen", String(Date.now()));
      } catch {
        // Without storage there's no digest; nothing else depends on it.
      }
    };
    mark();
    const tick = window.setInterval(() => document.hasFocus() && mark(), 60_000);
    window.addEventListener("blur", mark);
    document.addEventListener("visibilitychange", mark);
    return () => {
      window.clearInterval(tick);
      window.removeEventListener("blur", mark);
      document.removeEventListener("visibilitychange", mark);
    };
  }, []);
  return away;
}

/** What finished, failed or started waiting on you while you were away. */
function AwayDigest({ since, onOpen }: { since: number; onOpen: (id: string) => void }) {
  const q = useQuery({ queryKey: ["since", since], queryFn: () => api.since(since) });
  const list = q.data ?? [];
  if (!list.length) return null;
  const waiting = list.filter((s) => isAwaiting(s.status));
  const failed = list.filter((s) => s.status === "failed");
  const done = list.filter((s) => !isAwaiting(s.status));
  return (
    <Card title={`While you were away · ${duration(Date.now() - since)}`} className="mb-4">
      <p className="text-sm text-zinc-400">
        {done.length} finished{failed.length ? `, ${failed.length} failed` : ""}
        {waiting.length ? `, ${waiting.length - failed.length} waiting on you` : ""}.
      </p>
      <ul className="mt-2 divide-y divide-zinc-800/70">
        {[...waiting, ...done].slice(0, 8).map((s) => (
          <li key={s.id}>
            <button className="flex w-full items-center gap-3 py-1.5 text-left text-sm hover:text-zinc-100" onClick={() => onOpen(s.id)}>
              <StatusBadge status={s.status} size="xs" />
              <span className="min-w-0 flex-1 truncate text-zinc-300">{s.title ?? "Untitled session"}</span>
              <SourceBadge source={s.source} entrypoint={s.entrypoint} />
              <span className="shrink-0 text-xs text-zinc-500">{projectName(s.projectPath)}</span>
            </button>
          </li>
        ))}
      </ul>
    </Card>
  );
}
