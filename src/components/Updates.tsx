import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useMutation } from "@tanstack/react-query";
import { ChevronDown, Download, RotateCcw, X } from "lucide-react";
import { api, type ReleaseInfo, type UpdateProgress } from "../api";
import { date } from "../format";
import { Markdown } from "./Markdown";
import { Button, ErrorNote, cx } from "./ui";

const RELATION = {
  newer: { label: "Newer", className: "bg-emerald-500/15 text-emerald-300" },
  current: { label: "Installed", className: "bg-zinc-800 text-zinc-300" },
  previous: { label: "Previous", className: "bg-amber-500/15 text-amber-300" },
} as const;

/** True when a newer version can be installed from the app. */
export const hasUpdate = (releases: ReleaseInfo[] | undefined) => !!releases?.some((r) => r.relation === "newer" && r.installable);

/**
 * Pick a version to move to: every newer release, the installed one, and the
 * one before it to roll back. Installing downloads and verifies the signed
 * update, then restarts Tilly.
 */
export function Updates({ releases, loading, error, onRefresh, onClose }: { releases: ReleaseInfo[] | undefined; loading: boolean; error: unknown; onRefresh: () => void; onClose: () => void }) {
  const [confirm, setConfirm] = useState<string | null>(null);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const install = useMutation({ mutationFn: (version: string) => api.installRelease(version) });
  const busy = install.isPending;

  useEffect(() => {
    const un = listen<UpdateProgress>("update-progress", (e) => setProgress(e.payload));
    return () => void un.then((f) => f());
  }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !busy && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  const pct = progress?.total ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-6" role="dialog" aria-modal="true" aria-labelledby="updates-title">
      <button aria-label="Close" className="absolute inset-0 bg-black/50" onClick={() => !busy && onClose()} />
      <div className="relative flex max-h-full w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-zinc-800 bg-zinc-950 shadow-2xl">
        <div className="flex items-start justify-between gap-3 border-b border-zinc-800 px-6 py-4">
          <div>
            <h2 id="updates-title" className="text-base font-semibold text-zinc-50">Updates</h2>
            <p className="text-xs text-zinc-500">Install a newer version, or roll back to the previous one.</p>
          </div>
          <Button variant="ghost" size="sm" onClick={onClose} disabled={busy} aria-label="Close">
            <X size={16} />
          </Button>
        </div>

        <div className="space-y-3 overflow-y-auto px-6 py-5">
          {busy && (
            <div className="rounded-lg border border-zinc-800 p-3 text-sm text-zinc-300" role="status">
              <p>{pct == null ? "Downloading…" : pct < 100 ? `Downloading… ${pct}%` : "Installing; Tilly will restart."}</p>
              <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-800">
                <div className={cx("h-full bg-emerald-500 transition-all", pct == null && "w-1/3 animate-pulse")} style={pct == null ? undefined : { width: `${pct}%` }} />
              </div>
            </div>
          )}
          <ErrorNote error={install.error ?? error} />
          {loading && !releases && <p className="text-sm text-zinc-500">Checking GitHub for releases…</p>}
          {releases?.length === 0 && <p className="text-sm text-zinc-500">No releases found.</p>}
          {releases?.map((r) => {
            const tag = RELATION[r.relation];
            const action = r.relation === "newer" ? "Install" : r.relation === "previous" ? "Roll back" : null;
            return (
              <details key={r.version} className="group rounded-lg border border-zinc-800/80" open={r.relation !== "previous" && releases.indexOf(r) === 0}>
                <summary className="flex cursor-pointer list-none items-center gap-2 px-3 py-2.5">
                  <ChevronDown size={14} className="-rotate-90 text-zinc-500 transition-transform group-open:rotate-0" aria-hidden />
                  <span className="text-sm font-medium text-zinc-200">Version {r.version}</span>
                  <span className={cx("rounded px-1.5 py-0.5 text-[11px]", tag.className)}>{tag.label}</span>
                  {r.publishedAt && <span className="text-xs text-zinc-500">{date(Date.parse(r.publishedAt))}</span>}
                  <span className="flex-1" />
                  {action &&
                    (r.installable ? (
                      confirm === r.version ? (
                        <span className="flex items-center gap-1.5">
                          <Button size="sm" variant={r.relation === "newer" ? "primary" : "danger"} busy={busy} onClick={(e) => { e.preventDefault(); install.mutate(r.version); }}>
                            {r.relation === "newer" ? "Install and restart" : "Roll back and restart"}
                          </Button>
                          <Button size="sm" variant="ghost" disabled={busy} onClick={(e) => { e.preventDefault(); setConfirm(null); }}>Cancel</Button>
                        </span>
                      ) : (
                        <Button size="sm" variant={r.relation === "newer" ? "primary" : "secondary"} disabled={busy} onClick={(e) => { e.preventDefault(); setConfirm(r.version); }}>
                          {r.relation === "newer" ? <Download size={13} /> : <RotateCcw size={13} />} {action}
                        </Button>
                      )
                    ) : (
                      <span className="text-xs text-zinc-500" title={r.url}>Not available in the app</span>
                    ))}
                </summary>
                <div className="prose-md border-t border-zinc-800/80 px-4 py-3 text-sm text-zinc-300">
                  {r.notes.trim() ? <Markdown source={r.notes} /> : <p className="text-zinc-500">No notes.</p>}
                  {!r.installable && action && (
                    <p className="mt-2 text-xs text-zinc-500">This release has no signed update files, so download its installer from GitHub Releases: {r.url}</p>
                  )}
                </div>
              </details>
            );
          })}
        </div>

        <div className="flex items-center justify-between border-t border-zinc-800 px-6 py-3 text-xs text-zinc-500">
          <span>Updates are verified against Tilly's signing key before they install.</span>
          <Button size="sm" variant="secondary" busy={loading} disabled={busy} onClick={onRefresh}>Check again</Button>
        </div>
      </div>
    </div>
  );
}
