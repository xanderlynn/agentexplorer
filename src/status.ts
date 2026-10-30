import { Activity, CalendarClock, CircleCheck, CircleMinus, CircleStop, CircleX, Hourglass, MessageSquareReply, Moon, ShieldAlert, ShieldQuestion, type LucideIcon } from "lucide-react";
import type { LiveAgentStatus, LiveStatus } from "./api";
import { STATUS_GROUP, type SessionGroup } from "./generated";

/** Every status a session or sub-agent can show, and the ones you can give a sound or image. */
export type StatusKey = LiveStatus | "agent_done" | "agent_failed";

/** A session's group, from live.rs, plus `failed` for sub-agent failures. */
export type StatusGroup = SessionGroup | "failed";

export interface StatusToken {
  label: string;
  /** One line on what it means and how it is detected. */
  help: string;
  icon: LucideIcon;
  group: StatusGroup;
  /** Literal class names so Tailwind keeps them. */
  text: string;
  bg: string;
  ring: string;
  dot: string;
  /** A slow halo for statuses that want your attention. */
  pulse?: boolean;
}

export const STATUS: Record<StatusKey, StatusToken> = {
  asking: {
    label: "Your turn",
    help: "The agent finished and asked you something, or is waiting on a question or plan approval.",
    icon: MessageSquareReply,
    group: "awaiting",
    text: "text-amber-300",
    bg: "bg-amber-500/15",
    ring: "ring-amber-500/40",
    dot: "bg-amber-400",
    pulse: true,
  },
  needs_permission: {
    label: "Needs permission",
    help: "Waiting for you to approve a tool call (logged by Copilot).",
    icon: ShieldAlert,
    group: "awaiting",
    text: "text-orange-300",
    bg: "bg-orange-500/15",
    ring: "ring-orange-500/40",
    dot: "bg-orange-400",
    pulse: true,
  },
  maybe_blocked: {
    label: "Might need approval",
    help: "A file, web or MCP tool call has had no result for a while. Claude Code does not log permission prompts, so this is a best guess.",
    icon: ShieldQuestion,
    group: "awaiting",
    text: "text-orange-300",
    bg: "bg-orange-500/10",
    ring: "ring-orange-500/30",
    dot: "bg-orange-400",
    pulse: true,
  },
  working: {
    label: "Working",
    help: "Thinking, calling tools, or its sub-agents are at work.",
    icon: Activity,
    group: "active",
    text: "text-green-300",
    bg: "bg-green-500/15",
    ring: "ring-green-500/40",
    dot: "bg-green-400",
  },
  long_task: {
    label: "Long task",
    help: "Mid-turn but quiet for a few minutes: a long command, a build, or a sub-agent.",
    icon: Hourglass,
    group: "active",
    text: "text-green-300",
    bg: "bg-green-500/10",
    ring: "ring-green-500/30",
    dot: "bg-green-500/70",
  },
  done: {
    label: "Completed",
    help: "Finished its turn without asking you anything.",
    icon: CircleCheck,
    group: "completed",
    text: "text-sky-300",
    bg: "bg-sky-500/15",
    ring: "ring-sky-500/40",
    dot: "bg-sky-400",
  },
  ended: {
    label: "Closed",
    help: "The tool reported the session closed.",
    icon: CircleMinus,
    group: "completed",
    text: "text-zinc-300",
    bg: "bg-zinc-700/40",
    ring: "ring-zinc-500/40",
    dot: "bg-zinc-400",
  },
  failed: {
    label: "Failed",
    help: "The latest turn ended in an error or hit the output limit.",
    icon: CircleX,
    group: "awaiting",
    text: "text-rose-300",
    bg: "bg-rose-500/15",
    ring: "ring-rose-500/40",
    dot: "bg-rose-400",
  },
  scheduled: {
    label: "Scheduled",
    help: "Idle now, with work scheduled to run later.",
    icon: CalendarClock,
    group: "scheduled",
    text: "text-indigo-300",
    bg: "bg-indigo-500/15",
    ring: "ring-indigo-500/40",
    dot: "bg-indigo-400",
  },
  stale: {
    label: "Idle",
    help: "No activity for a long while; stalled, or the tool was closed.",
    icon: Moon,
    group: "idle",
    text: "text-zinc-400",
    bg: "bg-zinc-800/60",
    ring: "ring-zinc-600/40",
    dot: "bg-zinc-500",
  },
  agent_done: {
    label: "Sub-agent done",
    help: "A sub-agent finished and reported back.",
    icon: CircleCheck,
    group: "completed",
    text: "text-sky-300",
    bg: "bg-sky-500/15",
    ring: "ring-sky-500/40",
    dot: "bg-sky-400",
  },
  agent_failed: {
    label: "Sub-agent failed",
    help: "A sub-agent failed or stopped without reporting.",
    icon: CircleX,
    group: "failed",
    text: "text-rose-300",
    bg: "bg-rose-500/15",
    ring: "ring-rose-500/40",
    dot: "bg-rose-400",
  },
};

/** Sub-agent statuses on the same visual language. */
export const AGENT_STATUS: Record<LiveAgentStatus, StatusToken> = {
  running: STATUS.working,
  quiet: { ...STATUS.long_task, label: "Quiet", help: "Running, but silent for a few minutes." },
  done: { ...STATUS.done, label: "Done", help: "Finished and reported back." },
  failed: STATUS.failed,
  stopped: { ...STATUS.stale, label: "No report", help: "Never reported back.", icon: CircleStop },
};

/**
 * Most urgent first: what needs you, then what is running, then the rest.
 * Sorts the queue, picks the one sound in a burst, and colours the badge.
 */
const URGENCY: StatusKey[] = ["needs_permission", "asking", "failed", "maybe_blocked", "agent_failed", "working", "long_task", "scheduled", "done", "agent_done", "ended", "stale"];

export const urgency = (k: StatusKey) => URGENCY.indexOf(k);

/** The most urgent of some statuses. */
export const mostUrgent = <K extends StatusKey>(keys: K[]): K | undefined => [...keys].sort((a, b) => urgency(a) - urgency(b))[0];

export const isAwaiting = (s: LiveStatus) => STATUS_GROUP[s] === "awaiting";

/** Waiting on you now: awaiting and not parked (left for hours). */
export const waitingNow = (s: { status: LiveStatus; parked: boolean }) => isAwaiting(s.status) && !s.parked;
export const isActive = (s: LiveStatus) => STATUS_GROUP[s] === "active";
