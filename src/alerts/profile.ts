import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../api";
import type { StatusKey } from "../status";

/** The built-in alert tones, synthesised by the app (src-tauri/src/sound.rs). */
export const TONES = {
  chime: "Chime",
  ping: "Ping",
  knock: "Knock",
  rise: "Rise",
  fall: "Fall",
  soft: "Soft",
} as const;

export type Tone = keyof typeof TONES;

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

/**
 * Your sounds, images and notifications per status. The app owns the
 * defaults and plays the alerts (core status_alerts.rs); this is its shape.
 */
export interface AlertProfile {
  version: 1;
  volume: number;
  muted: boolean;
  quietHours: { enabled: boolean; start: string; end: string };
  /** Only play sounds while Governess is in the background. */
  onlyUnfocused: boolean;
  rules: Partial<Record<StatusKey, StatusRule>>;
}

const KEY = "alerts.profile";

const NO_RULE: StatusRule = { sound: null, image: null, notify: false, volume: 1 };

export function ruleFor(p: AlertProfile, key: StatusKey): StatusRule {
  return p.rules[key] ?? NO_RULE;
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

/** The profile, a setter that saves it, and a reset to the defaults. */
export function useProfile() {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["alertProfile"], queryFn: () => api.alertProfile(false) });
  const save = useMutation({
    mutationFn: async (p: AlertProfile) => {
      const before = q.data;
      await api.setSetting(KEY, JSON.stringify(p));
      // Files no rule uses any more are deleted, so replaced ones don't pile up.
      const kept = assetIds(p);
      if (before) await Promise.all([...assetIds(before)].filter((id) => !kept.has(id)).map((id) => api.deleteAlertAsset(id).catch(() => {})));
    },
    onMutate: (p) => qc.setQueryData(["alertProfile"], p),
    onSettled: () => qc.invalidateQueries({ queryKey: ["alertProfile"] }),
  });
  return {
    profile: q.data,
    save: (p: AlertProfile) => save.mutate(p),
    reset: async () => save.mutate(await api.alertProfile(true)),
  };
}
