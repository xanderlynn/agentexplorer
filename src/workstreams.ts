// Pure helpers for the Workstreams page, kept apart so they can be tested.
import type { Workstream, WsTask } from "./api";

/** Shares of the progress bar: done, in progress, and waiting on you. */
export function progress(ws: Workstream) {
  const total = ws.tasks.length;
  if (total === 0) return { done: 0, doing: 0, waiting: 0, total };
  const waiting = ws.tasks.filter((t) => t.kind === "human" && t.status !== "done").length;
  const done = ws.tasks.filter((t) => t.status === "done").length;
  const doing = ws.tasks.filter((t) => t.status === "in_progress" && t.kind === "agent").length;
  return { done: done / total, doing: doing / total, waiting: waiting / total, total };
}

/** What a task looks like in the tree. */
export type TaskState = "done" | "doing" | "yours" | "blocked" | "todo";

export function taskState(t: WsTask): TaskState {
  if (t.status === "done") return "done";
  if (t.kind === "human") return "yours";
  if (t.status === "blocked" || t.blockedBy.length > 0) return "blocked";
  if (t.status === "in_progress") return "doing";
  return "todo";
}

/**
 * The tree: each session with its tasks, in the order they were worked.
 * Sessions without a task list are left out, except in ones you made, where
 * every session you put in shows.
 */
export function tree(ws: Workstream) {
  return ws.sessions
    .map((s) => ({ session: s, tasks: ws.tasks.filter((t) => t.sessionId === s.id) }))
    .filter((n) => n.tasks.length > 0 || ws.custom || ws.sessions.length === 1);
}

/** Items from the tracker epic it was imported from, in the epic's order. */
export const trackerTasks = (ws: Workstream) => ws.tasks.filter((t) => !t.sessionId);

/** "GitHub" or "Azure DevOps", from an imported workstream's link. */
export function trackerName(url: string) {
  return /^https:\/\/github\.com\//.test(url) ? "GitHub" : "Azure DevOps";
}

/** Two-letter initials for an agent's avatar. */
export function initials(label: string) {
  const words = label.replace(/[^A-Za-z0-9 ]/g, " ").trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  return words.length === 1 ? words[0].slice(0, 2).replace(/^./, (c) => c.toUpperCase()) : (words[0][0] + words[1][0]).toUpperCase();
}

/** Starred first, then ones that need you, then the most recently active. */
export function sortForYou(list: Workstream[]) {
  return [...list].sort(
    (a, b) => Number(b.starred) - Number(a.starred) || Number(b.counts.needsYou > 0) - Number(a.counts.needsYou > 0) || b.lastActivity - a.lastActivity,
  );
}
