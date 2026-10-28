import type { StatusKey } from "../status";
import { playFile } from "./assets";
import { inQuietHours, ruleFor, type AlertProfile, type SoundChoice } from "./profile";
import { playTone } from "./tones";

let lastSound = 0;
/** Sounds closer together than this are dropped, so a burst stays one sound. */
const MIN_GAP_MS = 2000;

export function playChoice(choice: SoundChoice, volume: number) {
  if (!choice || volume <= 0) return;
  if (choice.kind === "tone") playTone(choice.tone, volume);
  else playFile(choice.id, volume).catch(() => {});
}

/** Plays the status's sound if the profile allows it right now. */
export function soundFor(p: AlertProfile, key: StatusKey) {
  const r = ruleFor(p, key);
  if (p.muted || !r.sound || inQuietHours(p)) return;
  if (p.onlyUnfocused && document.hasFocus()) return;
  const now = Date.now();
  if (now - lastSound < MIN_GAP_MS) return;
  lastSound = now;
  playChoice(r.sound, p.volume * r.volume);
}
