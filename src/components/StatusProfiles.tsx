import { useRef, useState } from "react";
import { ImagePlus, Play, Trash2, Upload } from "lucide-react";
import { api } from "../api";
import { DEFAULT_PROFILE, ruleFor, useProfile, type AlertProfile, type SoundChoice, type StatusRule } from "../alerts/profile";
import { playChoice } from "../alerts/player";
import { TONES, type Tone } from "../alerts/tones";
import { STATUS, type StatusKey } from "../status";
import { StatusAvatar } from "./status";
import { Button, Card, ErrorNote, cx, inputBase } from "./ui";

/** Statuses you can give a sound and image, most useful first. */
const MAIN: StatusKey[] = ["asking", "needs_permission", "maybe_blocked", "done", "failed", "agent_done", "agent_failed", "scheduled"];
const MORE: StatusKey[] = ["working", "long_task", "ended", "stale"];

/** Picks a local file and imports it; returns its id. */
function useImport(accept: string, onDone: (id: string) => void) {
  const input = useRef<HTMLInputElement>(null);
  const [error, setError] = useState<unknown>(null);
  const el = (
    <input
      ref={input}
      type="file"
      accept={accept}
      className="hidden"
      onChange={async (e) => {
        const f = e.target.files?.[0];
        e.target.value = "";
        if (!f) return;
        setError(null);
        try {
          const asset = await api.importAlertAsset(new Uint8Array(await f.arrayBuffer()));
          onDone(asset.id);
        } catch (err) {
          setError(err);
        }
      }}
    />
  );
  return { pick: () => input.current?.click(), el, error };
}

function ProfileCard({ status, profile, save }: { status: StatusKey; profile: AlertProfile; save: (p: AlertProfile) => void }) {
  const t = STATUS[status];
  const r = ruleFor(profile, status);
  const set = (patch: Partial<StatusRule>) => save({ ...profile, rules: { ...profile.rules, [status]: { ...r, ...patch } } });
  const image = useImport("image/png,image/jpeg,image/gif,image/webp", (id) => set({ image: id }));
  const sound = useImport("audio/wav,audio/mpeg,audio/ogg,.wav,.mp3,.ogg", (id) => set({ sound: { kind: "file", id } }));
  const value = r.sound == null ? "none" : r.sound.kind === "tone" ? r.sound.tone : "file";

  return (
    <li className={cx("flex flex-col gap-3 rounded-xl border bg-zinc-900/50 p-3", "border-zinc-800")}>
      <div className="flex items-center gap-3">
        <StatusAvatar status={status} size={48} />
        <div className="min-w-0">
          <div className={cx("text-sm font-medium", t.text)}>{t.label}</div>
          <div className="line-clamp-2 text-xs text-zinc-500">{t.help}</div>
        </div>
      </div>

      <div className="flex items-center gap-1.5">
        <Button size="sm" onClick={image.pick}>
          <ImagePlus size={13} /> {r.image ? "Change image" : "Add image"}
        </Button>
        {r.image && (
          <Button size="sm" variant="ghost" onClick={() => set({ image: null })} aria-label="Remove image">
            <Trash2 size={13} />
          </Button>
        )}
        {image.el}
      </div>

      <div className="flex items-center gap-1.5">
        <select
          className={cx(inputBase, "min-w-0 flex-1")}
          value={value}
          aria-label={`${t.label} sound`}
          onChange={(e) => {
            const v = e.target.value;
            if (v === "upload") return sound.pick();
            set({ sound: v === "none" ? null : v === "file" ? r.sound : ({ kind: "tone", tone: v as Tone } as SoundChoice) });
          }}
        >
          <option value="none">No sound</option>
          {(Object.keys(TONES) as Tone[]).map((k) => (
            <option key={k} value={k}>{TONES[k]}</option>
          ))}
          {r.sound?.kind === "file" && <option value="file">Your sound</option>}
          <option value="upload">Upload a sound…</option>
        </select>
        <Button size="sm" variant="ghost" disabled={!r.sound} onClick={() => playChoice(r.sound, profile.volume * r.volume)} aria-label={`Test ${t.label} sound`}>
          <Play size={13} />
        </Button>
        {sound.el}
      </div>

      <div className="flex items-center gap-2 text-xs text-zinc-400">
        <input
          type="range"
          min={0}
          max={1}
          step={0.05}
          value={r.volume}
          disabled={!r.sound}
          onChange={(e) => set({ volume: Number(e.target.value) })}
          className="min-w-0 flex-1 accent-emerald-500"
          aria-label={`${t.label} volume`}
        />
        <label className="flex shrink-0 items-center gap-1">
          <input type="checkbox" className="accent-emerald-500" checked={r.notify} onChange={(e) => set({ notify: e.target.checked })} aria-label={`Notify for ${t.label}`} />
          Notify
        </label>
      </div>
      <ErrorNote error={image.error ?? sound.error} />
    </li>
  );
}

/**
 * Settings › Status profiles: a sound, an image and a notification choice
 * for each status, with a master volume, mute and quiet hours.
 */
export function StatusProfiles() {
  const { profile, loaded, save } = useProfile();
  const [more, setMore] = useState(false);
  if (!loaded) return null;
  const q = profile.quietHours;
  return (
    <Card
      title="Status sounds and images"
      action={
        <Button size="sm" variant="ghost" onClick={() => save(DEFAULT_PROFILE)}>
          Reset to defaults
        </Button>
      }
    >
      <div className="mb-4 flex flex-wrap items-center gap-x-5 gap-y-2 text-sm text-zinc-300">
        <label className="flex items-center gap-2">
          Volume
          <input type="range" min={0} max={1} step={0.05} value={profile.volume} onChange={(e) => save({ ...profile, volume: Number(e.target.value) })} className="w-32 accent-emerald-500" />
          <span className="w-9 tabular-nums text-zinc-500">{Math.round(profile.volume * 100)}%</span>
        </label>
        <label className="flex items-center gap-1.5">
          <input type="checkbox" className="accent-emerald-500" checked={profile.muted} onChange={(e) => save({ ...profile, muted: e.target.checked })} />
          Mute all sounds
        </label>
        <label className="flex flex-wrap items-center gap-1.5">
          <input type="checkbox" className="accent-emerald-500" checked={q.enabled} onChange={(e) => save({ ...profile, quietHours: { ...q, enabled: e.target.checked } })} />
          Quiet hours
          <input type="time" className={cx(inputBase, "w-32")} value={q.start} disabled={!q.enabled} onChange={(e) => save({ ...profile, quietHours: { ...q, start: e.target.value } })} aria-label="Quiet hours start" />
          –
          <input type="time" className={cx(inputBase, "w-32")} value={q.end} disabled={!q.enabled} onChange={(e) => save({ ...profile, quietHours: { ...q, end: e.target.value } })} aria-label="Quiet hours end" />
        </label>
        <label className="flex items-center gap-1.5">
          <input type="checkbox" className="accent-emerald-500" checked={profile.onlyUnfocused} onChange={(e) => save({ ...profile, onlyUnfocused: e.target.checked })} />
          Only when Governess is in the background
        </label>
      </div>
      <ul className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        {(more ? [...MAIN, ...MORE] : MAIN).map((s) => (
          <ProfileCard key={s} status={s} profile={profile} save={save} />
        ))}
      </ul>
      <div className="mt-3 flex items-center justify-between text-xs text-zinc-500">
        <button type="button" className="hover:text-zinc-300" onClick={() => setMore(!more)}>
          {more ? "Fewer statuses" : `More statuses (${MORE.map((k) => STATUS[k].label).join(", ")})`}
        </button>
        <span className="inline-flex items-center gap-1">
          <Upload size={12} aria-hidden /> Images: PNG, JPEG, GIF, WebP up to 2 MB. Sounds: WAV, MP3, OGG up to 1 MB. Files stay on this computer.
        </span>
      </div>
    </Card>
  );
}
