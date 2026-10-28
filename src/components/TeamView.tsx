import { useMemo, useState } from "react";
import { CalendarClock, ChevronDown } from "lucide-react";
import type { ScheduledWork, SessionRow, Team, TeamAgent, TeamMessage } from "../api";
import { SOURCE_LABEL, ago, compact, dateTime, duration } from "../format";
import { AGENT_STATUS, STATUS } from "../status";
import { StatusBadge } from "./status";
import { Legend, cx } from "./ui";

/** A member of the session's team: you, the lead agent, or a sub-agent. */
interface Member {
  id: string;
  label: string;
  title: string;
  blurb: string;
  color: string;
  depth: number;
  agent?: TeamAgent;
}

const YOU = "you";
const LEAD = "lead";
const PALETTE = ["#38bdf8", "#a78bfa", "#f472b6", "#fb923c", "#facc15", "#2dd4bf", "#818cf8", "#f87171"];

const KIND_COLOR: Record<TeamMessage["kind"] | "ask", string> = {
  ask: "var(--color-zinc-400)",
  handoff: "var(--color-emerald-400)",
  report: "#38bdf8",
  message: "#fbbf24",
};
const KIND_VERB: Record<TeamMessage["kind"] | "ask", string> = {
  ask: "asked",
  handoff: "handed off",
  report: "reported",
  message: "messaged",
};

/** What each kind of agent is for, so the roster reads like a team. */
const ROLES: Record<string, [string, string]> = {
  "general-purpose": ["Generalist", "Multi-step research and changes"],
  explore: ["Scout", "Searches and reads the codebase"],
  plan: ["Architect", "Designs the approach before any code"],
  task: ["Runner", "Runs builds, tests and commands"],
  "code-review": ["Reviewer", "Checks changes for problems"],
  "rubber-duck": ["Sounding board", "Talks a problem through"],
  "claude-code-guide": ["Guide", "Answers questions about the tools"],
  fork: ["Fork", "A copy of the lead, with its context"],
};

export function roleOf(role: string): [string, string] {
  const key = role.toLowerCase().replace(/ agent$/, "").trim().replace(/\s+/g, "-");
  if (ROLES[key]) return ROLES[key];
  const words = role.replace(/[-_]/g, " ").replace(/ agent$/i, "");
  return [words.charAt(0).toUpperCase() + words.slice(1), "Custom agent"];
}

function members(team: Team, s: SessionRow): Member[] {
  const out: Member[] = [
    { id: YOU, label: "You", title: "Manager", blurb: "Sets the goal, reviews, unblocks", color: "var(--color-zinc-300)", depth: 0 },
    {
      id: LEAD,
      label: "Lead",
      title: SOURCE_LABEL[s.source] ?? "Lead agent",
      blurb: team.agents.length ? "Plans, hands off work, pulls it together" : "Did the work itself",
      color: "var(--color-emerald-500)",
      depth: 0,
    },
  ];
  // Each agent follows whoever handed it the work.
  const visit = (parent: string, depth: number) => {
    for (const a of team.agents.filter((a) => a.parent === parent)) {
      const [title, blurb] = roleOf(a.role);
      out.push({ id: a.id, label: a.name ?? title, title, blurb, color: PALETTE[(out.length - 2) % PALETTE.length], depth, agent: a });
      visit(a.id, depth + 1);
    }
  };
  visit(LEAD, 1);
  const known = new Set(out.map((m) => m.id));
  for (const a of team.agents.filter((a) => !known.has(a.id))) {
    const [title, blurb] = roleOf(a.role);
    out.push({ id: a.id, label: a.name ?? title, title, blurb, color: PALETTE[(out.length - 2) % PALETTE.length], depth: 1, agent: a });
  }
  return out;
}

/** Reads the common cron shapes aloud; anything else stays as written. */
export function cronText(cron: string): string {
  const [min, hour, dom, mon, dow] = cron.trim().split(/\s+/);
  const every = min?.match(/^\*\/(\d+)$/);
  if (every && hour === "*" && dom === "*" && mon === "*" && dow === "*") return `every ${every[1]} min`;
  if (/^\d+$/.test(min ?? "") && /^\d+$/.test(hour ?? "") && dom === "*" && mon === "*") {
    const at = `${hour.padStart(2, "0")}:${min.padStart(2, "0")}`;
    if (dow === "*") return `every day at ${at}`;
    if (dow === "1-5") return `weekdays at ${at}`;
  }
  return cron;
}

function upcoming(j: ScheduledWork, live: boolean) {
  return live && !j.cancelled && (j.recurring || (j.at != null && j.at > Date.now()));
}

/** The most agents at work at the same moment. */
function peakParallel(agents: TeamAgent[]): number {
  const edges = agents
    .filter((a) => a.startedAt != null)
    .flatMap((a) => [
      [a.startedAt!, 1],
      [a.endedAt ?? Date.now(), -1],
    ]);
  edges.sort((x, y) => x[0] - y[0] || x[1] - y[1]);
  let now = 0;
  let peak = 0;
  for (const [, d] of edges) peak = Math.max(peak, (now += d));
  return peak;
}

export function TeamView({
  team,
  session,
  prompts,
  totalTokens,
}: {
  team: Team;
  session: SessionRow;
  prompts: { ts: number | null; text: string }[];
  totalTokens: number;
}) {
  const crew = useMemo(() => members(team, session), [team, session]);
  const [focus, setFocus] = useState<string | null>(null);

  if (team.agents.length === 0 && team.scheduled.length === 0) {
    return (
      <div className="rounded-xl border border-zinc-800 bg-zinc-900/40 p-5 text-sm text-zinc-400">
        <div className="mb-1 font-medium text-zinc-200">The lead worked alone</div>
        No sub-agents were started and nothing was scheduled. Handing independent research, reviews or test runs to sub-agents keeps the lead's context small and lets the work run in parallel.
      </div>
    );
  }

  const agents = team.agents;
  const running = agents.filter((a) => a.status === "running").length;
  const unreported = agents.filter((a) => a.status === "failed" || a.status === "stopped").length;
  const agentTokens = agents.reduce((n, a) => n + a.tokens, 0);
  const next = team.scheduled.filter((j) => upcoming(j, team.live)).length;
  const roles = [...new Set(crew.slice(2).map((m) => m.title))];

  return (
    <div className="space-y-5">
      <section className="flex flex-wrap gap-x-5 gap-y-1 text-sm text-zinc-400">
        <span>
          <span className="text-zinc-100">{agents.length}</span> agent{agents.length === 1 ? "" : "s"}
          {roles.length > 0 && <span className="text-zinc-500"> · {roles.join(", ")}</span>}
        </span>
        {agents.length > 1 && (
          <span>
            peak <span className="text-zinc-100">{peakParallel(agents)}</span> at once
          </span>
        )}
        {running > 0 && <span className={STATUS.working.text}>{running} working now</span>}
        {unreported > 0 && <span className={STATUS.failed.text}>{unreported} never reported back</span>}
        {agentTokens > 0 && (
          <span title="Tokens the sub-agents used, compared with the whole session">
            {compact(agentTokens)} tokens
            {totalTokens > 0 && <span className="text-zinc-500"> · {Math.round((agentTokens / (agentTokens + totalTokens)) * 100)}% of all</span>}
          </span>
        )}
        {next > 0 && <span className={STATUS.scheduled.text}>{next} scheduled</span>}
      </section>

      <Roster crew={crew} model={session.model} focus={focus} setFocus={setFocus} />
      {agents.length > 0 && <Flow crew={crew} team={team} prompts={prompts} focus={focus} setFocus={setFocus} />}
      {agents.some((a) => a.startedAt != null) && <Timeline crew={crew} session={session} focus={focus} />}
      {team.scheduled.length > 0 && <Scheduled jobs={team.scheduled} live={team.live} crew={crew} />}
    </div>
  );
}

function Avatar({ m, size = 28 }: { m: Member; size?: number }) {
  return (
    <span
      className="inline-flex shrink-0 items-center justify-center rounded-full text-xs font-semibold text-zinc-950"
      style={{ background: m.color, width: size, height: size }}
      aria-hidden
    >
      {m.label.charAt(0).toUpperCase()}
    </span>
  );
}

function Roster({ crew, model, focus, setFocus }: { crew: Member[]; model: string | null; focus: string | null; setFocus: (id: string | null) => void }) {
  const byId = new Map(crew.map((m) => [m.id, m]));
  return (
    <section>
      <h3 className="mb-2 text-sm font-medium text-zinc-200">The team</h3>
      <ul className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
        {crew.map((m) => {
          const a = m.agent;
          const parent = a && a.parent !== LEAD ? byId.get(a.parent) : undefined;
          return (
            <li
              key={m.id}
              onMouseEnter={() => setFocus(m.id)}
              onMouseLeave={() => setFocus(null)}
              className={cx(
                "rounded-xl border bg-zinc-900/50 p-3 transition-colors",
                focus === m.id ? "border-zinc-500" : "border-zinc-800",
              )}
            >
              <div className="flex items-start gap-2.5">
                <Avatar m={m} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center justify-between gap-2">
                    <span className="truncate text-sm font-medium text-zinc-100">{m.label}</span>
                    {a && <StatusBadge token={AGENT_STATUS[a.status]} size="xs" />}
                  </div>
                  <div className="text-xs text-zinc-500">
                    {a && m.label === m.title ? a.role : m.title}
                    {a && m.label !== m.title && ` · ${a.role}`}
                    {parent && ` · from ${parent.label}`}
                    {m.id === LEAD && model && ` · ${model}`}
                  </div>
                </div>
              </div>
              <div className="mt-2 text-sm text-zinc-300">{a ? a.task || "No task description" : m.blurb}</div>
              {a && (
                <div className="mt-1.5 flex flex-wrap gap-x-2 text-xs text-zinc-500">
                  {a.model && <span>{a.model}</span>}
                  {a.startedAt != null && <span>{a.endedAt ? duration(a.endedAt - a.startedAt) : `started ${ago(Date.now() - a.startedAt)}`}</span>}
                  {a.toolCalls > 0 && <span>{a.toolCalls} tool calls</span>}
                  {a.tokens > 0 && <span>{compact(a.tokens)} tokens</span>}
                  {a.background && <span>background</span>}
                </div>
              )}
              {a && (a.brief || a.result) && (
                <details className="group mt-2 text-xs">
                  <summary className="flex cursor-pointer list-none items-center gap-1 text-zinc-400 hover:text-zinc-200">
                    <ChevronDown size={13} className="transition-transform group-open:rotate-180" /> Brief and report
                  </summary>
                  {a.brief && (
                    <div className="mt-1.5">
                      <div className="text-zinc-500">Asked to</div>
                      <div className="mt-0.5 max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded-md bg-zinc-950/60 p-2 text-zinc-300">{a.brief}</div>
                    </div>
                  )}
                  {a.result && (
                    <div className="mt-1.5">
                      <div className="text-zinc-500">Reported</div>
                      <div className="mt-0.5 max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded-md bg-zinc-950/60 p-2 text-zinc-300">{a.result}</div>
                    </div>
                  )}
                </details>
              )}
              {m.agent && Object.keys(m.agent.tools).length > 0 && (
                <div className="mt-2 flex flex-wrap gap-1">
                  {Object.entries(m.agent.tools)
                    .sort((x, y) => y[1] - x[1])
                    .slice(0, 5)
                    .map(([t, n]) => (
                      <span key={t} className="rounded bg-zinc-800/80 px-1 font-mono text-[11px] text-zinc-400">
                        {t} {n}
                      </span>
                    ))}
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

type Row = { ts: number | null; from: string; to: string; kind: TeamMessage["kind"] | "ask"; text: string };

const LANE = 56;
const ROW = 30;
const ROW_LIMIT = 160;

/**
 * A sequence diagram: one lane per member, one row per hand-off, report or
 * message, in time order. A member's lane is solid while it is working.
 */
function Flow({
  crew,
  team,
  prompts,
  focus,
  setFocus,
}: {
  crew: Member[];
  team: Team;
  prompts: { ts: number | null; text: string }[];
  focus: string | null;
  setFocus: (id: string | null) => void;
}) {
  const [withPrompts, setWithPrompts] = useState(prompts.length <= 30);
  const [all, setAll] = useState(false);

  const rows: Row[] = useMemo(() => {
    const asks: Row[] = withPrompts ? prompts.map((p) => ({ ts: p.ts, from: YOU, to: LEAD, kind: "ask", text: p.text })) : [];
    return [...asks, ...team.messages].sort((a, b) => (a.ts ?? Infinity) - (b.ts ?? Infinity));
  }, [team.messages, prompts, withPrompts]);

  const lane = new Map(crew.map((m, i) => [m.id, i]));
  // Rows during which each agent is at work: from its hand-off to its last report.
  const active = new Map<string, [number, number]>();
  rows.forEach((r, i) => {
    if (r.kind === "handoff" && !active.has(r.to)) active.set(r.to, [i, rows.length]);
    if (r.kind === "report" && active.has(r.from)) active.get(r.from)![1] = i;
  });
  for (const a of team.agents) {
    const span = active.get(a.id);
    if (span && a.status === "running") span[1] = rows.length;
  }

  const shown = all ? rows : rows.slice(0, ROW_LIMIT);
  const width = crew.length * LANE;
  const cx_ = (id: string) => (lane.get(id) ?? 0) * LANE + LANE / 2;
  const byId = new Map(crew.map((m) => [m.id, m]));
  const name = (id: string) => byId.get(id)?.label ?? id;

  return (
    <section>
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-medium text-zinc-200">How they worked together</h3>
        <label className="flex items-center gap-1.5 text-xs text-zinc-400">
          <input type="checkbox" className="accent-emerald-500" checked={withPrompts} onChange={(e) => setWithPrompts(e.target.checked)} />
          Show your prompts ({prompts.length})
        </label>
      </div>
      <div className="overflow-x-auto rounded-xl border border-zinc-800 bg-zinc-900/30">
        <div className="min-w-fit">
          <div className="sticky top-0 flex border-b border-zinc-800 bg-zinc-900/95 py-2">
            <div className="flex shrink-0" style={{ width }}>
              {crew.map((m) => (
                <button
                  key={m.id}
                  type="button"
                  className={cx("flex flex-col items-center gap-0.5 px-0.5", focus && focus !== m.id && "opacity-40")}
                  style={{ width: LANE }}
                  onMouseEnter={() => setFocus(m.id)}
                  onMouseLeave={() => setFocus(null)}
                  title={`${m.label}: ${m.title}`}
                >
                  <Avatar m={m} size={22} />
                  <span className="w-full truncate text-center text-[11px] text-zinc-400">{m.label}</span>
                </button>
              ))}
            </div>
            <div className="px-3 text-xs text-zinc-500">What was said</div>
          </div>
          <ol>
            {shown.map((r, i) => {
              const from = cx_(r.from);
              const to = lane.has(r.to) ? cx_(r.to) : from;
              const dim = focus != null && r.from !== focus && r.to !== focus;
              const color = KIND_COLOR[r.kind];
              const dir = to >= from ? 1 : -1;
              return (
                <li key={i} className={cx("flex items-center hover:bg-zinc-800/30", dim && "opacity-35")} style={{ height: ROW }}>
                  <svg width={width} height={ROW} className="shrink-0" aria-hidden>
                    {crew.map((m) => {
                      const x = cx_(m.id);
                      const span = active.get(m.id);
                      const busy = m.id === LEAD || (span != null && i >= span[0] && i <= span[1]);
                      return busy ? (
                        <rect key={m.id} x={x - 3} y={0} width={6} height={ROW} rx={1} fill={m.color} opacity={m.id === LEAD ? 0.35 : 0.55} />
                      ) : (
                        <line key={m.id} x1={x} x2={x} y1={0} y2={ROW} stroke="var(--color-zinc-700)" strokeDasharray="2 3" />
                      );
                    })}
                    {from === to ? (
                      <circle cx={from} cy={ROW / 2} r={4} fill={color} />
                    ) : (
                      <>
                        <line x1={from} x2={to - dir * 6} y1={ROW / 2} y2={ROW / 2} stroke={color} strokeWidth={1.75} strokeDasharray={r.kind === "report" ? "4 2" : undefined} />
                        <path d={`M ${to} ${ROW / 2} l ${-dir * 7} -4 v 8 z`} fill={color} />
                        <circle cx={from} cy={ROW / 2} r={2.5} fill={color} />
                      </>
                    )}
                  </svg>
                  <div className="flex min-w-0 flex-1 items-baseline gap-2 px-3 text-xs" title={r.text}>
                    <span className="w-14 shrink-0 tabular-nums text-zinc-500">{r.ts ? new Date(r.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : ""}</span>
                    <span className="shrink-0 text-zinc-300">
                      {name(r.from)} <span style={{ color }}>{KIND_VERB[r.kind]}</span>
                      {r.to !== r.from && <> {r.kind === "report" ? "to" : "→"} {name(r.to)}</>}
                    </span>
                    <span className="min-w-0 truncate text-zinc-500">{r.text}</span>
                  </div>
                </li>
              );
            })}
          </ol>
          {rows.length > shown.length && (
            <button type="button" className="w-full border-t border-zinc-800 py-2 text-xs text-zinc-400 hover:text-zinc-200" onClick={() => setAll(true)}>
              Show all {rows.length} steps
            </button>
          )}
        </div>
      </div>
      <div className="mt-2">
        <Legend
          items={[
            ["hand-off", KIND_COLOR.handoff],
            ["report back", KIND_COLOR.report],
            ["message", KIND_COLOR.message],
            ["your prompt", KIND_COLOR.ask],
          ]}
        />
      </div>
    </section>
  );
}

/** When each member was at work, on the session's clock. */
function Timeline({ crew, session, focus }: { crew: Member[]; session: SessionRow; focus: string | null }) {
  const agents = crew.filter((m) => m.agent?.startedAt != null);
  const start = Math.min(session.startedAt ?? Infinity, ...agents.map((m) => m.agent!.startedAt!));
  const end = Math.max(session.endedAt ?? 0, ...agents.map((m) => m.agent!.endedAt ?? Date.now()));
  const span = Math.max(1, end - start);
  const pct = (t: number) => `${((t - start) / span) * 100}%`;
  return (
    <section>
      <h3 className="mb-2 text-sm font-medium text-zinc-200">When each one worked</h3>
      <ul className="space-y-1">
        {[crew[1], ...agents].map((m) => {
          const a = m.agent;
          const from = a?.startedAt ?? start;
          const to = a ? (a.endedAt ?? (a.status === "running" ? Date.now() : from)) : end;
          return (
            <li key={m.id} className={cx("flex items-center gap-2", focus && focus !== m.id && "opacity-40")}>
              <span className="w-32 shrink-0 truncate text-xs text-zinc-300" style={{ paddingLeft: Math.max(0, m.depth - 1) * 10 }}>
                {m.label}
              </span>
              <span className="relative h-3.5 flex-1 rounded bg-zinc-800/60">
                <span
                  className={cx("absolute top-0.5 h-2.5 rounded-sm", a?.status === "running" && "motion-safe:animate-pulse")}
                  style={{ left: pct(from), width: `max(3px, ${((to - from) / span) * 100}%)`, background: m.color, opacity: m.id === LEAD ? 0.45 : 0.9 }}
                  title={`${m.label}: ${dateTime(from)}${a ? `, ${duration(to - from)}` : ""}`}
                />
              </span>
              <span className="w-12 shrink-0 text-right text-xs tabular-nums text-zinc-500">{a ? duration(to - from) : ""}</span>
            </li>
          );
        })}
      </ul>
      <div className="mt-1 flex justify-between pl-[8.5rem] pr-14 text-[11px] text-zinc-500">
        <span>{dateTime(start)}</span>
        <span>{duration(span)}</span>
      </div>
    </section>
  );
}

function Scheduled({ jobs, live, crew }: { jobs: ScheduledWork[]; live: boolean; crew: Member[] }) {
  const byId = new Map(crew.map((m) => [m.id, m.label]));
  return (
    <section>
      <h3 className="mb-2 text-sm font-medium text-zinc-200">Scheduled to run</h3>
      <ul className="space-y-1.5">
        {jobs.map((j, i) => {
          const soon = upcoming(j, live);
          const state = j.cancelled ? "cancelled" : soon ? (j.recurring ? "repeats" : "upcoming") : j.at && j.at <= Date.now() ? "due time passed" : "ended with the session";
          return (
            <li key={j.id ?? i} className="flex items-start gap-2.5 rounded-lg border border-zinc-800 bg-zinc-900/40 px-3 py-2">
              <CalendarClock size={15} className={cx("mt-0.5 shrink-0", soon ? STATUS.scheduled.text : "text-zinc-500")} />
              <div className="min-w-0 flex-1">
                <div className="text-sm text-zinc-200">{j.task || "Untitled job"}</div>
                <div className="mt-0.5 text-xs text-zinc-500">
                  {j.cron ? <span title={j.cron}>{cronText(j.cron)}</span> : j.at ? dateTime(j.at) : "time not recorded"}
                  {j.at && soon && ` · in ${duration(j.at - Date.now())}`}
                  {` · set by ${byId.get(j.by) ?? j.by}`}
                  {j.createdAt && ` ${ago(Date.now() - j.createdAt)}`}
                </div>
              </div>
              <span className={cx("shrink-0 rounded-md px-1.5 py-0.5 text-xs", soon ? cx(STATUS.scheduled.bg, STATUS.scheduled.text) : "bg-zinc-800 text-zinc-400")}>{state}</span>
            </li>
          );
        })}
      </ul>
      {!live && <p className="mt-1.5 text-xs text-zinc-500">Jobs set up inside a session stop when it ends.</p>}
    </section>
  );
}
