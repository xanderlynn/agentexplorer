import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ClipboardList, CornerDownLeft, FileText, LayoutGrid } from "lucide-react";
import { api } from "../api";
import type { Page } from "../App";
import { dateTime, projectName } from "../format";
import { cx } from "./ui";

type Item = { key: string; label: string; detail?: string; icon: typeof FileText; run: () => void };

/** ⌘K: jump to a page, run an action, or find any session by its prompts. */
export function CommandPalette({
  onClose,
  go,
  openSession,
  plan,
  pages,
}: {
  onClose: () => void;
  go: (p: Page) => void;
  openSession: (id: string) => void;
  plan: () => void;
  pages: { page: Page; label: string }[];
}) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const [debounced, setDebounced] = useState("");
  useEffect(() => {
    const t = setTimeout(() => setDebounced(q.trim()), 200);
    return () => clearTimeout(t);
  }, [q]);
  const sessions = useQuery({
    queryKey: ["palette", debounced],
    queryFn: () => api.sessions({ days: 0, search: debounced }),
    enabled: debounced.length >= 2,
  });

  const items = useMemo<Item[]>(() => {
    const needle = q.trim().toLowerCase();
    const done = (f: () => void) => () => {
      f();
      onClose();
    };
    const actions: Item[] = [
      { key: "a:plan", label: "Plan a task", detail: "pre-flight card", icon: ClipboardList, run: done(plan) },
      { key: "a:retro", label: "Generate weekly retro", icon: ClipboardList, run: done(() => go("retro")) },
      { key: "a:exp", label: "New experiment", icon: ClipboardList, run: done(() => go("experiments")) },
      { key: "a:settings", label: "Settings", icon: ClipboardList, run: done(() => go("settings")) },
    ];
    const pageItems: Item[] = pages.map((p) => ({ key: `p:${p.page}`, label: p.label, detail: "page", icon: LayoutGrid, run: done(() => go(p.page)) }));
    const matched = [...actions, ...pageItems].filter((i) => !needle || i.label.toLowerCase().includes(needle));
    const found: Item[] = (debounced.length >= 2 ? sessions.data ?? [] : []).slice(0, 8).map((s) => ({
      key: `s:${s.id}`,
      label: s.title ?? "Untitled session",
      detail: `${projectName(s.repoRoot ?? s.projectPath)} · ${dateTime(s.startedAt)}`,
      icon: FileText,
      run: done(() => openSession(s.id)),
    }));
    return [...matched, ...found];
  }, [q, debounced, sessions.data, pages, go, openSession, plan, onClose]);

  useEffect(() => setSel(0), [q]);

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") onClose();
    else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((i) => Math.min(items.length - 1, i + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((i) => Math.max(0, i - 1));
    } else if (e.key === "Enter") items[sel]?.run();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[12vh]" onMouseDown={onClose}>
      <div className="w-full max-w-xl overflow-hidden rounded-xl border border-zinc-700 bg-zinc-900 shadow-2xl" role="dialog" aria-label="Command palette" onMouseDown={(e) => e.stopPropagation()}>
        <input
          autoFocus
          className="w-full border-b border-zinc-800 bg-transparent px-4 py-3 text-sm text-zinc-100 placeholder:text-zinc-500 focus:outline-none"
          placeholder="Search pages, actions, or any prompt you've written…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={onKey}
          aria-label="Search"
        />
        <ul className="max-h-[50vh] overflow-y-auto py-1" role="listbox">
          {items.length === 0 && <li className="px-4 py-3 text-sm text-zinc-500">{debounced.length >= 2 && sessions.isFetching ? "Searching…" : "No matches"}</li>}
          {items.map((it, i) => (
            <li key={it.key} role="option" aria-selected={i === sel}>
              <button
                className={cx("flex w-full items-center gap-3 px-4 py-2 text-left text-sm", i === sel ? "bg-zinc-800 text-zinc-50" : "text-zinc-300")}
                onMouseEnter={() => setSel(i)}
                onClick={it.run}
              >
                <it.icon size={15} className="shrink-0 text-zinc-500" aria-hidden />
                <span className="min-w-0 flex-1 truncate">{it.label}</span>
                {it.detail && <span className="shrink-0 truncate text-xs text-zinc-500">{it.detail}</span>}
                {i === sel && <CornerDownLeft size={13} className="shrink-0 text-zinc-500" aria-hidden />}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
