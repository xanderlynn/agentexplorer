import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { BookOpenCheck, Gauge, ListChecks, NotebookPen, Radio, Settings as SettingsIcon, Sparkles, Trophy } from "lucide-react";
import { api, type LiveState } from "./api";
import { cx } from "./components/ui";
import { SessionDrawer } from "./components/SessionDrawer";
import { projectName } from "./format";
import Overview from "./pages/Overview";
import Sessions from "./pages/Sessions";
import Live from "./pages/Live";
import Journal from "./pages/Journal";
import Playbooks from "./pages/Playbooks";
import Scorecard from "./pages/Scorecard";
import Retro from "./pages/Retro";
import Settings from "./pages/Settings";

export type Page = "overview" | "live" | "sessions" | "journal" | "playbooks" | "scorecard" | "retro" | "settings";

const NAV: { id: Page; label: string; icon: typeof Gauge }[] = [
  { id: "overview", label: "Overview", icon: Gauge },
  { id: "live", label: "Live", icon: Radio },
  { id: "sessions", label: "Sessions", icon: ListChecks },
  { id: "journal", label: "Journal", icon: NotebookPen },
  { id: "playbooks", label: "Playbooks", icon: BookOpenCheck },
  { id: "scorecard", label: "Scorecard", icon: Trophy },
  { id: "retro", label: "Weekly retro", icon: Sparkles },
  { id: "settings", label: "Settings", icon: SettingsIcon },
];

const NEEDS_YOU: LiveState[] = ["waiting", "needs_you"];

function readStored<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw == null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

function store(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage can be unavailable; the app works without it.
  }
}

export default function App() {
  const qc = useQueryClient();
  const [page, setPage] = useState<Page>(() => readStored("page", "overview"));
  const [days, setDays] = useState<number>(() => readStored("days", 30));
  const [openSession, setOpenSession] = useState<string | null>(null);

  useEffect(() => store("page", page), [page]);
  useEffect(() => store("days", days), [days]);

  // The Rust ingester emits this after every pass that changed something.
  useEffect(() => {
    const un = listen("data-changed", () => qc.invalidateQueries());
    return () => {
      un.then((f) => f());
    };
  }, [qc]);

  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  // Keep polling while the window is hidden: that is when a notification matters.
  const live = useQuery({ queryKey: ["live"], queryFn: api.live, refetchInterval: 5000, refetchIntervalInBackground: true, staleTime: 0 });
  useWaitingNotifications(live.data?.sessions, settings.data?.values["notify.waiting"] !== "false");

  const waiting = live.data?.waitingOnYou ?? 0;
  const go = (p: Page) => setPage(p);

  return (
    <div className="flex h-full">
      <aside className="flex w-52 shrink-0 flex-col border-r border-zinc-800/80 bg-zinc-950 px-3 py-4">
        <div className="mb-6 px-2">
          <div className="text-sm font-semibold tracking-tight text-zinc-50">Execution Ledger</div>
          <div className="text-xs text-zinc-500">Get better at delegating to AI</div>
        </div>
        <nav className="flex flex-col gap-0.5">
          {NAV.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              onClick={() => go(id)}
              className={cx(
                "flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-sm transition-colors",
                page === id ? "bg-zinc-800 text-zinc-50" : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200",
              )}
            >
              <Icon size={16} />
              <span className="flex-1">{label}</span>
              {id === "live" && waiting > 0 && (
                <span className="rounded-full bg-amber-500/20 px-1.5 text-xs font-semibold text-amber-300">{waiting}</span>
              )}
            </button>
          ))}
        </nav>
        <div className="mt-auto px-2 text-[11px] leading-relaxed text-zinc-600">
          All data stays on this computer, except a retro you choose to generate.
        </div>
      </aside>

      <main className="min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-7xl px-6 py-6 lg:px-8">
          {page === "overview" && <Overview days={days} setDays={setDays} onOpen={setOpenSession} go={go} />}
          {page === "live" && <Live onOpen={setOpenSession} />}
          {page === "sessions" && <Sessions days={days} setDays={setDays} onOpen={setOpenSession} />}
          {page === "journal" && <Journal days={days} setDays={setDays} onOpen={setOpenSession} />}
          {page === "playbooks" && <Playbooks />}
          {page === "scorecard" && <Scorecard days={days} setDays={setDays} />}
          {page === "retro" && <Retro go={go} />}
          {page === "settings" && <Settings />}
        </div>
      </main>

      {openSession && <SessionDrawer id={openSession} onClose={() => setOpenSession(null)} />}
    </div>
  );
}

// Sends a desktop notification when a session starts waiting on the human.
function useWaitingNotifications(sessions: { id: string; state: LiveState; title: string | null; projectPath: string | null }[] | undefined, enabled: boolean) {
  const previous = useRef<Map<string, LiveState> | null>(null);
  useEffect(() => {
    if (!sessions) return;
    const now = new Map(sessions.map((s) => [s.id, s.state]));
    const before = previous.current;
    previous.current = now;
    // The first poll establishes a baseline; don't notify for old sessions.
    if (!before || !enabled) return;
    const fresh = sessions.filter((s) => NEEDS_YOU.includes(s.state) && !NEEDS_YOU.includes(before.get(s.id) ?? "idle"));
    if (fresh.length === 0) return;
    (async () => {
      let ok = await isPermissionGranted();
      if (!ok) ok = (await requestPermission()) === "granted";
      if (!ok) return;
      for (const s of fresh.slice(0, 3)) {
        sendNotification({
          title: s.state === "needs_you" ? "Agent needs permission" : "Agent is waiting on you",
          body: `${projectName(s.projectPath)} · ${s.title ?? "Untitled session"}`,
        });
      }
    })().catch(() => {});
  }, [sessions, enabled]);
}
