import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { Bot, ChartColumn, Coins, Gauge, ListChecks, Search, Settings as SettingsIcon, Sparkles, Sun, Trophy, X } from "lucide-react";
import { useLive, type LiveSession, type LiveStatus, type LiveSummary, type SessionFilter } from "./api";
import { STATUS, isActive, isAwaiting, mostUrgent } from "./status";
import { cx } from "./components/ui";
import { Logo } from "./components/Logo";
import { SessionDrawer } from "./components/SessionDrawer";
import { Preflight } from "./components/Preflight";
import { QuickReview } from "./components/QuickReview";
import { CommandPalette } from "./components/CommandPalette";
import { projectName } from "./format";
import Today from "./pages/Today";
import Experiments from "./pages/Experiments";
import Execution from "./pages/Execution";
import Overview from "./pages/Overview";
import Sessions from "./pages/Sessions";
import ControlCenter from "./pages/ControlCenter";
import Teams from "./pages/Teams";
import Toolkit from "./pages/Toolkit";
import Journal from "./pages/Journal";
import Patterns from "./pages/Patterns";
import Usage from "./pages/Usage";
import Playbooks from "./pages/Playbooks";
import Scorecard from "./pages/Scorecard";
import Retro from "./pages/Retro";
import Settings from "./pages/Settings";

/** A drill-through into Sessions: filters plus a label for the chip. */
export type Drill = Omit<SessionFilter, "days"> & { label: string };

export type Page = "today" | "experiments" | "execution" | "overview" | "control" | "teams" | "toolkit" | "sessions" | "patterns" | "usage" | "journal" | "playbooks" | "scorecard" | "retro" | "settings";

/**
 * Six areas, each with one or more tabs. Pages keep their ids so links and
 * drill-throughs work unchanged; the sidebar shows the area, the tab bar
 * the page.
 */
const GROUPS: { id: string; label: string; icon: typeof Gauge; tabs: { page: Page; label: string }[] }[] = [
  { id: "agents", label: "Agents", icon: Bot, tabs: [{ page: "control", label: "Control center" }, { page: "teams", label: "Teams" }] },
  { id: "today", label: "Today", icon: Sun, tabs: [{ page: "today", label: "Today" }] },
  { id: "sessions", label: "Sessions", icon: ListChecks, tabs: [{ page: "sessions", label: "Sessions" }] },
  { id: "outcomes", label: "Outcomes", icon: Trophy, tabs: [{ page: "overview", label: "Summary" }, { page: "scorecard", label: "Delivery & reality" }] },
  {
    id: "habits",
    label: "Habits",
    icon: ChartColumn,
    tabs: [
      { page: "patterns", label: "Patterns" },
      { page: "execution", label: "Execution" },
      { page: "toolkit", label: "Toolkit" },
      { page: "journal", label: "Journal" },
      { page: "playbooks", label: "Playbooks" },
    ],
  },
  { id: "usage", label: "Usage", icon: Coins, tabs: [{ page: "usage", label: "Usage" }] },
  { id: "improve", label: "Improve", icon: Sparkles, tabs: [{ page: "retro", label: "Weekly retro" }, { page: "experiments", label: "Experiments & goals" }] },
];

const groupOf = (p: Page) => GROUPS.find((g) => g.tabs.some((t) => t.page === p));


/** Queries that show live state and refetch as soon as data changes. */
const LIVE_QUERIES = new Set(["teams", "session"]);

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
  // "live" was the control center's id before it replaced the Live page.
  const [page, setPage] = useState<Page>(() => {
    const p = readStored<string>("page", "control");
    return (p === "live" ? "control" : p) as Page;
  });
  const [planning, setPlanning] = useState(false);
  const [palette, setPalette] = useState(false);
  const group = groupOf(page);

  // ⌘K / Ctrl+K opens the command palette from anywhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const [days, setDays] = useState<number>(() => readStored("days", 30));
  const [openSession, setOpenSession] = useState<string | null>(null);
  const [drill, setDrill] = useState<Drill | null>(null);
  const openSessions = (d: Drill) => {
    setDrill(d);
    setPage("sessions");
  };

  useEffect(() => store("page", page), [page]);
  useEffect(() => store("days", days), [days]);

  // The Rust ingester emits this after every pass that changed something.
  // Live views refetch now; everything else is marked stale and refetches
  // the next time it is shown, so a busy agent doesn't re-run every query.
  useEffect(() => {
    // The live summary arrives whole whenever it changes.
    const unLive = listen<LiveSummary>("live", (e) => qc.setQueryData(["live"], e.payload));
    const un = listen("data-changed", () => {
      qc.invalidateQueries({ predicate: (q) => LIVE_QUERIES.has(String(q.queryKey[0])) });
      qc.invalidateQueries({ predicate: (q) => !LIVE_QUERIES.has(String(q.queryKey[0])), refetchType: "none" });
    });
    return () => {
      unLive.then((f) => f());
      un.then((f) => f());
    };
  }, [qc]);

  // Sounds and notifications are played by the app itself (see
  // src-tauri/src/background.rs), so they work with this window closed.
  const live = useLive();
  // Finished sessions move to completedToday; the review prompt watches both.
  const tracked = useMemo(() => live.data && [...live.data.sessions, ...live.data.completedToday], [live.data]);

  const awaiting = live.data?.sessions.filter((s) => isAwaiting(s.status)) ?? [];
  // The badge takes the most urgent waiting status's colour.
  const urgent = STATUS[mostUrgent(awaiting.map((s) => s.status)) ?? "asking"];
  const go = (p: Page) => {
    setDrill(null);
    setPage(p);
  };

  return (
    <div className="flex h-full">
      <aside className="flex w-52 shrink-0 flex-col border-r border-zinc-800/80 bg-zinc-950 px-3 py-4">
        <div className="mb-6 flex items-center gap-2.5 px-2">
          <Logo />
          <div>
            <div className="text-sm font-semibold tracking-tight text-zinc-50">Tilly</div>
            <div className="text-xs text-zinc-500">Keeps your agents in line</div>
          </div>
        </div>
        <nav className="flex flex-col gap-0.5">
          {GROUPS.map(({ id, label, icon: Icon, tabs }) => (
            <button
              key={id}
              onClick={() => go(tabs[0].page)}
              className={cx(
                "flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-sm transition-colors",
                group?.id === id ? "bg-zinc-800 text-zinc-50" : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200",
              )}
            >
              <Icon size={16} aria-hidden />
              <span className="flex-1">{label}</span>
              {id === "agents" && awaiting.length > 0 && (
                <span className={cx("rounded-full px-1.5 text-xs font-semibold", urgent.bg, urgent.text)} aria-label={`${awaiting.length} awaiting you`}>
                  {awaiting.length}
                </span>
              )}
            </button>
          ))}
        </nav>
        <button
          onClick={() => setPalette(true)}
          className="mt-4 flex items-center justify-between rounded-lg border border-zinc-800 px-2.5 py-1.5 text-xs text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200"
        >
          <span className="inline-flex items-center gap-2">
            <Search size={13} aria-hidden /> Search
          </span>
          <kbd className="font-sans text-zinc-500">⌘K</kbd>
        </button>
        <div className="mt-auto space-y-3">
          <button
            onClick={() => go("settings")}
            className={cx(
              "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-sm transition-colors",
              page === "settings" ? "bg-zinc-800 text-zinc-50" : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200",
            )}
          >
            <SettingsIcon size={16} aria-hidden /> Settings
          </button>
          <div className="px-2 text-xs leading-relaxed text-zinc-500">All data stays on this computer, except a retro you choose to generate.</div>
        </div>
      </aside>

      <main className="min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-7xl px-6 py-6 lg:px-8">
          {group && group.tabs.length > 1 && (
            <div className="mb-5 flex gap-1 border-b border-zinc-800" role="tablist">
              {group.tabs.map((t) => (
                <button
                  key={t.page}
                  role="tab"
                  aria-selected={page === t.page}
                  onClick={() => go(t.page)}
                  className={cx(
                    "-mb-px border-b-2 px-3 py-2 text-sm transition-colors",
                    page === t.page ? "border-emerald-500 text-zinc-50" : "border-transparent text-zinc-400 hover:text-zinc-200",
                  )}
                >
                  {t.label}
                </button>
              ))}
            </div>
          )}
          {page === "today" && <Today onOpen={setOpenSession} go={go} drill={openSessions} plan={() => setPlanning(true)} />}
          {page === "experiments" && <Experiments drill={openSessions} />}
          {page === "execution" && <Execution days={days} setDays={setDays} drill={openSessions} />}
          {page === "overview" && <Overview days={days} setDays={setDays} onOpen={setOpenSession} go={go} drill={openSessions} />}
          {page === "control" && <ControlCenter onOpen={setOpenSession} />}
          {page === "teams" && <Teams onOpen={setOpenSession} />}
          {page === "toolkit" && <Toolkit days={days} setDays={setDays} />}
          {page === "sessions" && <Sessions days={days} setDays={setDays} onOpen={setOpenSession} drill={drill} clearDrill={() => setDrill(null)} />}
          {page === "patterns" && <Patterns days={days} setDays={setDays} onOpen={setOpenSession} drill={openSessions} />}
          {page === "usage" && <Usage days={days} setDays={setDays} drill={openSessions} />}
          {page === "journal" && <Journal days={days} setDays={setDays} onOpen={setOpenSession} />}
          {page === "playbooks" && <Playbooks />}
          {page === "scorecard" && <Scorecard days={days} setDays={setDays} drill={openSessions} />}
          {page === "retro" && <Retro go={go} />}
          {page === "settings" && <Settings />}
        </div>
      </main>

      {openSession && (
        <SessionDrawer id={openSession} onClose={() => setOpenSession(null)} />
      )}
      {planning && <Preflight onClose={() => setPlanning(false)} />}
      {palette && (
        <CommandPalette
          onClose={() => setPalette(false)}
          go={go}
          openSession={setOpenSession}
          plan={() => setPlanning(true)}
          pages={GROUPS.flatMap((g) => g.tabs.map((t) => ({ page: t.page, label: t.label === g.label ? t.label : `${g.label} › ${t.label}` })))}
        />
      )}
      <ReviewPrompt sessions={tracked} onOpen={setOpenSession} />
    </div>
  );
}

const PROMPTS_PER_DAY = 3;

/**
 * Asks for a one-tap review when a session finishes, while it's fresh.
 * Never more than three a day, and never for a session already rated.
 */
function ReviewPrompt({ sessions, onOpen }: { sessions: LiveSession[] | undefined; onOpen: (id: string) => void }) {
  const previous = useRef<Map<string, LiveStatus> | null>(null);
  const [queue, setQueue] = useState<LiveSession[]>([]);
  useEffect(() => {
    if (!sessions) return;
    const now = new Map(sessions.map((s) => [s.id, s.status]));
    const before = previous.current;
    previous.current = now;
    if (!before) return;
    const finished = sessions.filter((s) => {
      const was = before.get(s.id);
      const active = was != null && (isActive(was) || isAwaiting(was));
      return active && (s.status === "ended" || s.status === "stale") && !s.outcome;
    });
    if (!finished.length) return;
    const today = new Date().toDateString();
    const shown = readStored<{ day: string; n: number }>("reviewPrompts", { day: today, n: 0 });
    const used = shown.day === today ? shown.n : 0;
    const room = Math.max(0, PROMPTS_PER_DAY - used);
    if (!room) return;
    const add = finished.slice(0, room);
    store("reviewPrompts", { day: today, n: used + add.length });
    setQueue((q) => [...q, ...add.filter((s) => !q.some((x) => x.id === s.id))]);
  }, [sessions]);

  const current = queue[0];
  if (!current) return null;
  const dismiss = () => setQueue((q) => q.slice(1));
  return (
    <div className="fixed bottom-5 right-5 z-30 w-96 rounded-xl border border-zinc-700 bg-zinc-900 p-4 shadow-2xl" role="dialog" aria-label="Review the session that just finished">
      <div className="mb-2 flex items-start justify-between gap-2">
        <button className="min-w-0 text-left" onClick={() => onOpen(current.id)}>
          <div className="text-xs text-zinc-500">Session finished · {projectName(current.projectPath)}</div>
          <div className="truncate text-sm font-medium text-zinc-100">{current.title ?? "Untitled session"}</div>
        </button>
        <button aria-label="Dismiss" className="text-zinc-500 hover:text-zinc-200" onClick={dismiss}>
          <X size={15} />
        </button>
      </div>
      <QuickReview key={current.id} id={current.id} onDone={dismiss} compact />
    </div>
  );
}
