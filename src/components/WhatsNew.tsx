import { useEffect, useState } from "react";
import { ChevronDown, X } from "lucide-react";
// The same file the GitHub releases use, bundled at build time.
import changelog from "../../CHANGELOG.md?raw";
import { Markdown } from "./Markdown";
import { Button } from "./ui";

export interface Release {
  version: string;
  date: string | null;
  notes: string;
}

/** Released versions from CHANGELOG.md, newest first. Unreleased is left out. */
export function releases(text = changelog): Release[] {
  const parts = text.split(/^## \[/m).slice(1);
  return parts
    .map((part) => {
      const [heading, ...body] = part.split("\n");
      const [, version, date] = heading.match(/^([^\]]+)\](?:\s*-\s*(.+))?/) ?? [];
      // Link references at the end of the file are not notes.
      const notes = body.join("\n").replace(/^\[[^\]]+\]: .*$/gm, "").trim();
      return { version, date: date?.trim() ?? null, notes };
    })
    .filter((r) => r.version && r.version !== "Unreleased" && r.notes);
}

/** The newest version with notes: what this build is. */
export const CURRENT_VERSION = releases()[0]?.version ?? "";

const SEEN_KEY = "whatsNewSeen";

/**
 * Marks What's new as unseen after each update (a dot in the sidebar) and
 * opens it only when asked; it never opens by itself.
 */
export function useWhatsNew() {
  const [open, setOpen] = useState(false);
  const [unseen, setUnseen] = useState(false);
  useEffect(() => {
    let seen: string | null = null;
    try {
      seen = localStorage.getItem(SEEN_KEY);
    } catch {
      // Storage can be unavailable; then it just doesn't open by itself.
      return;
    }
    if (CURRENT_VERSION && seen !== CURRENT_VERSION) setUnseen(true);
  }, []);
  const close = () => {
    setOpen(false);
    setUnseen(false);
    try {
      localStorage.setItem(SEEN_KEY, CURRENT_VERSION);
    } catch {
      // Without storage it may show again next launch.
    }
  };
  return { open, unseen, show: () => setOpen(true), close };
}

/** The release notes: the newest version open, earlier ones folded. */
export function WhatsNew({ onClose }: { onClose: () => void }) {
  const all = releases();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-6" role="dialog" aria-modal="true" aria-labelledby="whats-new">
      <button aria-label="Close" className="absolute inset-0 bg-black/50" onClick={onClose} />
      <div className="relative flex max-h-full w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-zinc-800 bg-zinc-950 shadow-2xl">
        <div className="flex items-start justify-between gap-3 border-b border-zinc-800 px-6 py-4">
          <div>
            <h2 id="whats-new" className="text-base font-semibold text-zinc-50">What's new in Tilly</h2>
            <p className="text-xs text-zinc-500">
              Version {CURRENT_VERSION}
              {all[0]?.date && ` · ${all[0].date}`}
            </p>
          </div>
          <Button variant="ghost" size="sm" onClick={onClose} aria-label="Close">
            <X size={16} />
          </Button>
        </div>
        <div className="overflow-y-auto px-6 py-5">
          {all.length === 0 ? (
            <p className="text-sm text-zinc-500">No release notes yet.</p>
          ) : (
            <div className="space-y-5">
              <article className="prose-md text-sm text-zinc-300">
                <Markdown source={all[0].notes} />
              </article>
              {all.slice(1).map((r) => (
                <details key={r.version} className="group rounded-lg border border-zinc-800/80">
                  <summary className="flex cursor-pointer list-none items-center gap-1.5 px-3 py-2 text-sm text-zinc-300 hover:text-zinc-100">
                    <ChevronDown size={14} className="-rotate-90 transition-transform group-open:rotate-0" aria-hidden />
                    Version {r.version}
                    {r.date && <span className="text-xs text-zinc-500">· {r.date}</span>}
                  </summary>
                  <div className="prose-md border-t border-zinc-800/80 px-4 py-3 text-sm text-zinc-300">
                    <Markdown source={r.notes} />
                  </div>
                </details>
              ))}
            </div>
          )}
        </div>
        <div className="flex items-center justify-between border-t border-zinc-800 px-6 py-3 text-xs text-zinc-500">
          <span>Full history in CHANGELOG.md and on GitHub Releases.</span>
          <Button variant="primary" size="sm" onClick={onClose}>
            Got it
          </Button>
        </div>
      </div>
    </div>
  );
}
