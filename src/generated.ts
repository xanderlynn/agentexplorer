// Generated from the Rust engine by src-tauri/core/src/codegen.rs. Do not edit;
// run `UPDATE_GENERATED=1 cargo test -p tilly-core generated_ts` in src-tauri.

export type Source = "claude_code" | "copilot_cli" | "copilot_vscode";

/** Every supported tool, in the order Settings lists them. */
export const SOURCES: { id: Source; label: string; shortLabel: string }[] = [
  { id: "claude_code", label: "Claude Code", shortLabel: "Claude Code" },
  { id: "copilot_cli", label: "Copilot CLI", shortLabel: "Copilot CLI" },
  { id: "copilot_vscode", label: "Copilot Chat (VS Code)", shortLabel: "Copilot Chat" },
];

/** One status per session; see live.rs `status_for`. */
export type LiveStatus = "working" | "long_task" | "maybe_blocked" | "needs_permission" | "asking" | "done" | "failed" | "scheduled" | "stale" | "ended";

/** The part of the control center a status belongs to. */
export type SessionGroup = "active" | "awaiting" | "completed" | "idle" | "scheduled";

export const STATUS_GROUP: Record<LiveStatus, SessionGroup> = {
  working: "active",
  long_task: "active",
  maybe_blocked: "awaiting",
  needs_permission: "awaiting",
  asking: "awaiting",
  done: "completed",
  failed: "awaiting",
  scheduled: "scheduled",
  stale: "idle",
  ended: "completed",
};

export type LiveAgentStatus = "running" | "quiet" | "done" | "failed" | "stopped";

export type TaskStatus = "pending" | "in_progress" | "done" | "cancelled";

/** Who a task is for; see workstreams.rs `classify`. */
export type TaskKind = "agent" | "human";

/** How a command failed; see util.rs `ErrorKind`. */
export type ErrorKind = "not_found" | "busy" | "failed";

/** Below this many samples a rate is noise: shown greyed out or not at all. */
export const MIN_SAMPLE = 5;
