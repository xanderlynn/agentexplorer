import { useRef, useState, type ReactNode } from "react";
import { ImagePlus, Play, RotateCcw, X } from "lucide-react";
import { api } from "../api";
import { DEFAULT_PROFILE, ruleFor, useProfile, type AlertProfile, type SoundChoice, type StatusRule } from "../alerts/profile";
import { playChoice } from "../alerts/player";
import { TONES, type Tone } from "../alerts/tones";
import { STATUS, type StatusKey } from "../status";
import { StatusAvatar } from "./status";
import { Button, Card, ErrorNote, cx, inputBase } from "./ui";

/** Statuses grouped the way you meet them; the last group starts folded. */
const GROUPS: { title: string; keys: StatusKey[]; folded?: boolean }[] = [
  { title: "Awaiting you", keys: ["asking", "needs_permission", "maybe_blocked", "failed"] },
  { title: "Finished", keys: ["done", "scheduled"] },
  { title: "Sub-agents", keys: ["agent_done", "agent_failed"] },
  { title: "Everything else", keys: ["working", "long_task", "ended", "stale"], folded: true },
];

/** One column template for the header and every row, so they line up. */
const COLS = "md:grid md:grid-cols-[minmax(0,1.6fr)_5.5rem_minmax(11rem,1fr)_minmax(8rem,0.7fr)_4.5rem] md:items-center md:gap-4";

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

/** A label shown above each control only when rows stack on narrow windows. */
function Cell({ label, children, className }: { label: string; children: ReactNode; className?: string }) {
  return (
    <div className={cx("flex min-w-0 items-center gap-2", className)}>
      <span className="w-16 shrink-0 text-xs text-zinc-500 md:hidden">{label}</span>
      {children}
    </div>
  );
}

function Row({ status, profile, save }: { status: StatusKey; profile: AlertProfile; save: (p: AlertProfile) => void }) {
  const t = STATUS[status];
  const r = ruleFor(profile, status);
  const set = (patch: Partial<StatusRule>) => save({ ...profile, rules: { ...profile.rules, [status]: { ...r, ...patch } } });
  const image = useImport("image/png,image/jpeg,image/gif,image/webp", (id) => set({ image: id }));
  const sound = useImport("audio/wav,audio/mpeg,audio/ogg,.wav,.mp3,.ogg", (id) => set({ sound: { kind: "file", id } }));
  const value = r.sound == null ? "none" : r.sound.kind === "tone" ? r.sound.tone : "file";

  return (
    <li className="py-3">
      <div className={cx("space-y-2 md:space-y-0", COLS)}>
        <div className="flex min-w-0 items-center gap-3">
          <StatusAvatar status={status} size={36} />
          <div className="min-w-0">
            <div className={cx("text-sm font-medium", t.text)}>{t.label}</div>
            <div className="truncate text-xs text-zinc-500" title={t.help}>{t.help}</div>
          </div>
        </div>

        <Cell label="Image">
          <button
            type="button"
            onClick={image.pick}
            className="inline-flex h-8 items-center gap-1.5 rounded-md border border-zinc-700 px-2 text-xs text-zinc-300 hover:bg-zinc-800"
            title={r.image ? "Change image" : "Add an image"}
          >
            <ImagePlus size={13} aria-hidden /> {r.image ? "Change" : "Add"}
          </button>
          {r.image && (
            <button type="button" onClick={() => set({ image: null })} className="rounded p-1 text-zinc-500 hover:text-zinc-200" aria-label={`Remove ${t.label} image`}>
              <X size={13} />
            </button>
          )}
          {image.el}
        </Cell>

        <Cell label="Sound">
          <select
            className={cx(inputBase, "h-8 min-w-0 flex-1")}
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
          <button
            type="button"
            disabled={!r.sound}
            onClick={() => playChoice(r.sound, profile.volume * r.volume)}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-zinc-700 text-zinc-300 hover:bg-zinc-800 disabled:opacity-30"
            aria-label={`Play ${t.label} sound`}
          >
            <Play size={13} />
          </button>
          {sound.el}
        </Cell>

        <Cell label="Volume">
          <input
            type="range"
            min={0}
            max={1}
            step={0.05}
            value={r.volume}
            disabled={!r.sound}
            onChange={(e) => set({ volume: Number(e.target.value) })}
            className="min-w-0 flex-1 accent-emerald-500 disabled:opacity-30"
            aria-label={`${t.label} volume`}
          />
          <span className={cx("w-9 shrink-0 text-right text-xs tabular-nums", r.sound ? "text-zinc-400" : "text-zinc-600")}>{Math.round(r.volume * 100)}%</span>
        </Cell>

        <Cell label="Notify" className="md:justify-center">
          <Switch checked={r.notify} onChange={(on) => set({ notify: on })} label={`Desktop notification for ${t.label}`} />
        </Cell>
      </div>
      <ErrorNote error={image.error ?? sound.error} />
    </li>
  );
}

function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (on: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cx("relative h-5 w-9 shrink-0 rounded-full transition-colors disabled:opacity-40", checked ? "bg-emerald-500" : "bg-zinc-700")}
    >
      <span className={cx("absolute top-0.5 h-4 w-4 rounded-full bg-zinc-50 shadow transition-transform", checked ? "translate-x-4" : "translate-x-0.5")} />
    </button>
  );
}

/** A labelled group in the toolbar. */
function Control({ label, children, grow }: { label: string; children: ReactNode; grow?: boolean }) {
  return (
    <div className={cx("flex flex-col gap-1.5", grow && "min-w-[14rem] flex-1")}>
      <span className="text-xs font-medium uppercase tracking-wide text-zinc-500">{label}</span>
      <div className="flex h-8 items-center gap-2">{children}</div>
    </div>
  );
}

/**
 * Settings › Status sounds and images: one row per status with its image,
 * sound, volume and notification, under a toolbar for volume, mute and
 * quiet hours.
 */
export function StatusProfiles() {
  const { profile, loaded, save } = useProfile();
  const [open, setOpen] = useState(false);
  if (!loaded) return null;
  const q = profile.quietHours;
  return (
    <Card
      className="xl:col-span-2"
      title="Status sounds and images"
      action={
        <Button size="sm" variant="ghost" onClick={() => save(DEFAULT_PROFILE)}>
          <RotateCcw size={13} /> Reset to defaults
        </Button>
      }
    >
      <div className="flex flex-wrap gap-x-8 gap-y-3 rounded-lg border border-zinc-800 bg-zinc-950/40 px-4 py-3">
        <Control label="Master volume" grow>
          <input type="range" min={0} max={1} step={0.05} value={profile.volume} onChange={(e) => save({ ...profile, volume: Number(e.target.value) })} className="min-w-0 flex-1 accent-emerald-500" aria-label="Master volume" />
          <span className="w-9 text-right text-xs tabular-nums text-zinc-400">{Math.round(profile.volume * 100)}%</span>
        </Control>
        <Control label="Mute all sounds">
          <Switch checked={profile.muted} onChange={(on) => save({ ...profile, muted: on })} label="Mute all sounds" />
          <span className="text-xs text-zinc-500">{profile.muted ? "Muted" : "Sounds on"}</span>
        </Control>
        <Control label="Quiet hours">
          <Switch checked={q.enabled} onChange={(on) => save({ ...profile, quietHours: { ...q, enabled: on } })} label="Quiet hours" />
          <input type="time" className={cx(inputBase, "h-8 w-[8rem] px-2")} value={q.start} disabled={!q.enabled} onChange={(e) => save({ ...profile, quietHours: { ...q, start: e.target.value } })} aria-label="Quiet hours start" />
          <span className="text-xs text-zinc-500">to</span>
          <input type="time" className={cx(inputBase, "h-8 w-[8rem] px-2")} value={q.end} disabled={!q.enabled} onChange={(e) => save({ ...profile, quietHours: { ...q, end: e.target.value } })} aria-label="Quiet hours end" />
        </Control>
        <Control label="Only in the background">
          <Switch checked={profile.onlyUnfocused} onChange={(on) => save({ ...profile, onlyUnfocused: on })} label="Only play sounds while Governess is in the background" />
          <span className="text-xs text-zinc-500">Silent while you're looking</span>
        </Control>
      </div>

      <div className={cx("mt-4 hidden border-b border-zinc-800 pb-2 text-xs font-medium uppercase tracking-wide text-zinc-500", COLS)}>
        <span>Status</span>
        <span>Image</span>
        <span>Sound</span>
        <span>Volume</span>
        <span className="text-center">Notify</span>
      </div>

      {GROUPS.map((g) => {
        const shown = !g.folded || open;
        return (
          <section key={g.title} className="mt-3">
            {g.folded ? (
              <button type="button" onClick={() => setOpen(!open)} className="text-xs font-medium text-zinc-400 hover:text-zinc-200">
                {open ? "Hide" : "Show"} {g.title.toLowerCase()} ({g.keys.map((k) => STATUS[k].label).join(", ")})
              </button>
            ) : (
              <h4 className="text-xs font-medium text-zinc-400">{g.title}</h4>
            )}
            {shown && (
              <ul className="divide-y divide-zinc-800/70">
                {g.keys.map((s) => (
                  <Row key={s} status={s} profile={profile} save={save} />
                ))}
              </ul>
            )}
          </section>
        );
      })}

      <p className="mt-4 border-t border-zinc-800 pt-3 text-xs text-zinc-500">
        Images: PNG, JPEG, GIF or WebP up to 2 MB. Sounds: WAV, MP3 or OGG up to 1 MB. Files stay on this computer. Quiet hours hold notifications as well as sounds.
      </p>
    </Card>
  );
}
