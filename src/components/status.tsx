import { useEffect, useState } from "react";
import type { LiveAgent } from "../api";
import { imageUrl } from "../alerts/assets";
import { ruleFor, useProfile } from "../alerts/profile";
import { agentName } from "../format";
import { AGENT_STATUS, STATUS, type StatusKey, type StatusToken } from "../status";
import { cx } from "./ui";

/** Icon and label together: status never rests on colour alone. */
export function StatusBadge({ status, token, size = "sm", label = true }: { status?: StatusKey; token?: StatusToken; size?: "xs" | "sm"; label?: boolean }) {
  const t = token ?? STATUS[status!];
  const Icon = t.icon;
  return (
    <span
      className={cx("inline-flex shrink-0 items-center gap-1 rounded-md font-medium ring-1 ring-inset", t.bg, t.text, t.ring, size === "xs" ? "px-1 py-px text-[11px]" : "px-1.5 py-0.5 text-xs")}
      title={t.help}
    >
      <Icon size={size === "xs" ? 11 : 13} aria-hidden className={cx(t.pulse && "motion-safe:animate-[pulse_2.4s_ease-in-out_infinite]")} />
      {label ? t.label : <span className="sr-only">{t.label}</span>}
    </span>
  );
}

/** A status dot with a halo for statuses that want you. */
export function StatusDot({ token }: { token: StatusToken }) {
  return (
    <span className="relative inline-flex h-2.5 w-2.5 shrink-0" title={token.label} aria-label={token.label} role="img">
      {token.pulse && <span className={cx("absolute -inset-0.5 rounded-full opacity-40 motion-safe:animate-[pulse_2.4s_ease-in-out_infinite]", token.dot)} />}
      <span className={cx("relative h-2.5 w-2.5 rounded-full", token.dot)} />
    </span>
  );
}

const reduceMotion = () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

function useImage(id: string | null) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setUrl(null);
    // An animated GIF moves; with reduced motion the icon stands in.
    if (id && !(id.endsWith(".gif") && reduceMotion())) imageUrl(id).then((u) => live && setUrl(u), () => {});
    return () => {
      live = false;
    };
  }, [id]);
  return url;
}

/**
 * The status as an avatar: your own image for it when you set one, else
 * its icon. The icon stays as a corner badge, so the image is decoration.
 */
export function StatusAvatar({ status, size = 40 }: { status: StatusKey; size?: number }) {
  const { profile } = useProfile();
  const image = useImage(profile ? ruleFor(profile, status).image : null);
  const [broken, setBroken] = useState<string | null>(null);
  const url = image && image !== broken ? image : null;
  const t = STATUS[status];
  const Icon = t.icon;
  return (
    <span className="relative inline-flex shrink-0" style={{ width: size, height: size }} title={t.label} role="img" aria-label={t.label}>
      {url ? (
        <img src={url} alt="" className="h-full w-full rounded-full object-cover ring-2 ring-inset ring-zinc-900" onError={() => setBroken(url)} />
      ) : (
        <span className={cx("flex h-full w-full items-center justify-center rounded-full ring-1 ring-inset", t.bg, t.ring, t.text)}>
          <Icon size={Math.round(size * 0.5)} aria-hidden />
        </span>
      )}
      {url && (
        <span className={cx("absolute -bottom-0.5 -right-0.5 flex items-center justify-center rounded-full ring-2 ring-zinc-950", t.bg, t.text)} style={{ width: size * 0.42, height: size * 0.42 }}>
          <Icon size={Math.round(size * 0.26)} aria-hidden />
        </span>
      )}
    </span>
  );
}

/** The lead and its sub-agents as a row of small avatars, each with its status. */
export function TeamStack({ agents, max = 6 }: { agents: LiveAgent[]; max?: number }) {
  if (agents.length === 0) return null;
  const shown = agents.slice(0, max);
  return (
    <span className="flex items-center gap-1" role="list" aria-label={`${agents.length} sub-agents`}>
      {shown.map((a) => {
        const t = AGENT_STATUS[a.status];
        const Icon = t.icon;
        const who = agentName(a);
        return (
          <span key={a.id} role="listitem" className={cx("relative flex h-5 w-5 items-center justify-center rounded-full text-[10px] font-semibold ring-1 ring-inset", t.bg, t.text, t.ring)} title={`${who} · ${t.label}: ${a.task}`}>
            <span aria-hidden>{who.charAt(0).toUpperCase()}</span>
            <span className="sr-only">{`${who}: ${t.label}`}</span>
            <Icon size={9} aria-hidden className="absolute -bottom-0.5 -right-0.5 rounded-full bg-zinc-950" />
          </span>
        );
      })}
      {agents.length > max && <span className="text-xs text-zinc-500">+{agents.length - max}</span>}
    </span>
  );
}
