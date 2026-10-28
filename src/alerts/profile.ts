import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../api";
import type { StatusKey } from "../status";
import type { Tone } from "./tones";

export type SoundChoice = { kind: "tone"; tone: Tone } | { kind: "file"; id: string } | null;

export interface StatusRule {
  sound: SoundChoice;
  /** An imported image id, shown for this status. */
  image: string | null;
  /** Also send a desktop notification. */
  notify: boolean;
  /** 0–1, multiplied by the master volume. */
  volume: number;
}

export interface AlertProfile {
  version: 1;
  volume: number;
  muted: boolean;
  quietHours: { enabled: boolean; start: string; end: string };
  /** Only play sounds while Governess is in the background. */
  onlyUnfocused: boolean;
  rules: Partial<Record<StatusKey, StatusRule>>;
}

const rule = (sound: SoundChoice, notify: boolean): StatusRule => ({ sound, image: null, notify, volume: 1 });

export const DEFAULT_PROFILE: AlertProfile = {
  version: 1,
  volume: 0.7,
  muted: false,
  quietHours: { enabled: false, start: "22:00", end: "08:00" },
  onlyUnfocused: false,
  rules: {
    asking: rule({ kind: "tone", tone: "chime" }, true),
    needs_permission: rule({ kind: "tone", tone: "knock" }, true),
    maybe_blocked: rule({ kind: "tone", tone: "ping" }, false),
    done: rule({ kind: "tone", tone: "rise" }, false),
    failed: rule({ kind: "tone", tone: "fall" }, true),
    agent_failed: rule(null, false),
    agent_done: rule(null, false),
  },
};

export const KEY = "alerts.profile";

export function parseProfile(raw: string | undefined): AlertProfile {
  if (!raw) return DEFAULT_PROFILE;
  try {
    const p = JSON.parse(raw) as Partial<AlertProfile>;
    return p.version === 1 ? { ...DEFAULT_PROFILE, ...p, quietHours: { ...DEFAULT_PROFILE.quietHours, ...p.quietHours } } : DEFAULT_PROFILE;
  } catch {
    return DEFAULT_PROFILE;
  }
}

/** Every imported file a profile refers to. */
function assetIds(p: AlertProfile): Set<string> {
  const ids = new Set<string>();
  for (const r of Object.values(p.rules)) {
    if (r?.image) ids.add(r.image);
    if (r?.sound?.kind === "file") ids.add(r.sound.id);
  }
  return ids;
}

export function ruleFor(p: AlertProfile, key: StatusKey): StatusRule {
  return p.rules[key] ?? rule(null, false);
}

/** The profile from settings, and a setter that saves it. */
export function useProfile() {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const profile = parseProfile(settings.data?.values[KEY]);
  const save = useMutation({
    mutationFn: async (p: AlertProfile) => {
      await api.setSetting(KEY, JSON.stringify(p));
      // Files no rule uses any more are deleted, so replaced ones don't pile up.
      const kept = assetIds(p);
      await Promise.all([...assetIds(profile)].filter((id) => !kept.has(id)).map((id) => api.deleteAlertAsset(id).catch(() => {})));
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["settings"] }),
  });
  return { profile, loaded: settings.isSuccess, save: (p: AlertProfile) => save.mutate(p) };
}

/** True inside the quiet hours, which may wrap past midnight. */
export function inQuietHours(p: AlertProfile, now = new Date()): boolean {
  if (!p.quietHours.enabled) return false;
  const m = (s: string) => {
    const [h, mm] = s.split(":").map(Number);
    return (h || 0) * 60 + (mm || 0);
  };
  const t = now.getHours() * 60 + now.getMinutes();
  const [a, b] = [m(p.quietHours.start), m(p.quietHours.end)];
  return a <= b ? t >= a && t < b : t >= a || t < b;
}
