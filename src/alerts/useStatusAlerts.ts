import { useEffect, useRef } from "react";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import type { LiveAgentStatus, LiveSession, LiveStatus } from "../api";
import { projectName } from "../format";
import { STATUS, mostUrgent, type StatusKey } from "../status";
import { inQuietHours, ruleFor, type AlertProfile } from "./profile";
import { soundFor } from "./player";

interface Seen {
  /** The status we have announced. */
  status: LiveStatus;
  /** A new status seen once; announced when the next poll agrees. */
  pending: LiveStatus | null;
  agents: Map<string, LiveAgentStatus>;
  /** Polls since it was last in the list; dropped after a while. */
  missing: number;
}

/** Forget a session after this many polls without it (about 5 minutes). */
const FORGET_AFTER = 60;


/** Notifications at once beyond this are folded into "and N more". */
const MAX_NOTES = 3;

/**
 * Plays the profile's sound and sends a notification once per status
 * change. The first poll is a baseline (no alerts for sessions already
 * running), and a change must hold for two polls, so a session that flips
 * between turns in a few seconds stays quiet.
 */
export function useStatusAlerts(sessions: LiveSession[] | undefined, profile: AlertProfile, notifications: boolean) {
  const seen = useRef<Map<string, Seen> | null>(null);
  const profileRef = useRef(profile);
  profileRef.current = profile;

  useEffect(() => {
    if (!sessions) return;
    const first = seen.current == null;
    const map = (seen.current ??= new Map());
    const fired: { key: StatusKey; s: LiveSession; detail?: string }[] = [];

    const present = new Set(sessions.map((s) => s.id));
    for (const [id, x] of map) if (!present.has(id) && ++x.missing > FORGET_AFTER) map.delete(id);

    for (const s of sessions) {
      let prev = map.get(s.id);
      const agents = new Map(s.agents.map((a) => [a.id, a.status]));
      if (first) {
        map.set(s.id, { status: s.status, pending: null, agents, missing: 0 });
        continue;
      }
      // A session that shows up after the first poll comes from idle, so
      // one that appears already waiting on you still alerts.
      prev ??= { status: "stale", pending: null, agents, missing: 0 };
      map.set(s.id, prev);
      prev.missing = 0;
      if (s.status === prev.status) prev.pending = null;
      else if (prev.pending === s.status) {
        prev.status = s.status;
        prev.pending = null;
        fired.push({ key: s.status, s });
      } else prev.pending = s.status;

      for (const a of s.agents) {
        const was = prev.agents.get(a.id);
        if (was === a.status || was === "done" || was === "failed" || was === "stopped") continue;
        if (a.status === "done") fired.push({ key: "agent_done", s, detail: a.name ?? a.role });
        // Stopped means the session ended without a report, not a failure.
        if (a.status === "failed") fired.push({ key: "agent_failed", s, detail: a.name ?? a.role });
      }
      prev.agents = agents;
    }
    if (fired.length === 0) return;

    const p = profileRef.current;
    // The most urgent status with a sound wins; the rest of a burst is dropped.
    const loudest = mostUrgent(fired.filter((f) => ruleFor(p, f.key).sound).map((f) => f.key));
    if (loudest) soundFor(p, loudest);

    const notes = fired.filter((f) => ruleFor(p, f.key).notify);
    // Quiet hours hold notifications too; mute is for sounds only.
    if (!notifications || notes.length === 0 || inQuietHours(p)) return;
    (async () => {
      let ok = await isPermissionGranted();
      if (!ok) ok = (await requestPermission()) === "granted";
      if (!ok) return;
      for (const f of notes.slice(0, MAX_NOTES)) {
        sendNotification({
          title: f.detail ? `${STATUS[f.key].label}: ${f.detail}` : STATUS[f.key].label,
          body: `${projectName(f.s.projectPath)} · ${f.s.title ?? "Untitled session"}`,
        });
      }
      if (notes.length > MAX_NOTES) sendNotification({ title: `and ${notes.length - MAX_NOTES} more`, body: "Open Governess to see them all." });
    })().catch(() => {});
  }, [sessions, notifications]);
}
