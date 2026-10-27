import { useQuery } from "@tanstack/react-query";
import { ClipboardList, Flag, FlaskConical, Target } from "lucide-react";
import { api, isWaiting, useLive } from "../api";
import type { Drill, Page } from "../App";
import { QuickReview } from "../components/QuickReview";
import { Swimlanes } from "../components/Swimlanes";
import { Button, Card, Loading, PageHeader, SourceBadge, cx } from "../components/ui";
import { dateTime, duration, projectName } from "../format";
import { fmtMetric } from "../metrics";
import { InsightItem } from "../components/InsightItem";

/**
 * The landing page: what needs you now, yesterday's sessions to rate in a
 * tap, the experiment you're running, your goals, and your planned tasks.
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
  const midnight = new Date();
  midnight.setHours(0, 0, 0, 0);
  const since = midnight.getTime();
  const lanes = useQuery({ queryKey: ["timelines", since], queryFn: () => api.timelines(since), refetchInterval: 30_000 });
  const unusual = useQuery({ queryKey: ["anomalies", 14], queryFn: () => api.anomalies(14) });

  const waiting = live.data?.sessions.filter((s) => isWaiting(s.state)) ?? [];
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

      {waiting.length > 0 && (
        <Card title={`Waiting on you (${waiting.length})`} className="mb-4 border-amber-500/30">
          <ul className="divide-y divide-zinc-800/70">
            {waiting.map((s) => (
              <li key={s.id}>
                <button className="flex w-full items-start gap-3 py-2 text-left hover:bg-zinc-800/30" onClick={() => onOpen(s.id)}>
                  <span className={cx("mt-1.5 h-2.5 w-2.5 shrink-0 rounded-full", s.state === "needs_you" ? "bg-rose-400" : "bg-amber-400")} aria-hidden />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm text-zinc-100">{s.title ?? "Untitled"}</span>
                    <span className="block text-xs text-zinc-500">
                      {s.state === "needs_you" ? "Needs permission" : "Your turn"} for {duration(s.ageMs)} · {projectName(s.projectPath)}
                    </span>
                    {s.lastMessage && <span className="mt-1 line-clamp-2 block text-xs text-zinc-400">{s.lastMessage}</span>}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </Card>
      )}

      {lanes.data && lanes.data.length > 0 && (
        <Card title="Today's agents" className="mb-4">
          <Swimlanes lanes={lanes.data} since={since} onOpen={onOpen} />
        </Card>
      )}

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
        <Card title={`Rate recent sessions (${toRate.data?.length ?? 0})`} className="xl:col-span-2" action={toRate.data?.length ? <button className="text-xs text-zinc-400 hover:text-zinc-200" onClick={() => drill({ label: "unrated sessions", outcome: "unrated" })}>All unrated →</button> : undefined}>
          {toRate.isLoading && <Loading />}
          {toRate.data?.length === 0 && <p className="text-sm text-zinc-500">All caught up.</p>}
          <ul className="divide-y divide-zinc-800/70">
            {toRate.data?.slice(0, 6).map((s) => (
              <li key={s.id} className="py-3">
                <button className="mb-2 block w-full text-left" onClick={() => onOpen(s.id)}>
                  <span className="block truncate text-sm text-zinc-100">{s.title ?? "Untitled"}</span>
                  <span className="flex items-center gap-2 text-xs text-zinc-500">
                    <SourceBadge source={s.source} /> · {projectName(s.repoRoot ?? s.projectPath)} · {duration(s.activeMs)} · {dateTime(s.startedAt)}
                    {s.commits > 0 && ` · ${s.commits} commits`}
                  </span>
                </button>
                <QuickReview id={s.id} compact />
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
