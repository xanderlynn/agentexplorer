// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
// Field names mirror the Rust structs (serde camelCase).
import { invoke, type InvokeArgs } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import type { AlertProfile, SoundChoice } from "./alerts/profile";

import type { ErrorKind, LiveAgentStatus, LiveStatus, Source, TaskKind, TaskStatus } from "./generated";
export type { ErrorKind, LiveAgentStatus, LiveStatus, Source, TaskKind, TaskStatus };

/** A failed command. `kind` tells a missing or busy thing from a real failure; showing it gives the message. */
export class CommandError extends Error {
  constructor(readonly kind: ErrorKind, message: string) {
    super(message);
    this.name = "CommandError";
  }
  override toString() {
    return this.message;
  }
}

/** Calls a Rust command, turning its structured error into a CommandError. */
async function call<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    if (e && typeof e === "object" && "kind" in e && "message" in e) throw new CommandError(e.kind as ErrorKind, String(e.message));
    throw e;
  }
}
export type Outcome = "shipped" | "partial" | "abandoned" | "redo";
export const OUTCOMES: Outcome[] = ["shipped", "partial", "abandoned", "redo"];

export interface SessionRow {
  id: string;
  source: Source;
  /** Which app ran it: "cli", "claude-desktop", "copilot-app", "vscode"… */
  entrypoint: string | null;
  /** Pull requests it opened or worked on. */
  prs: string[];
  /** For an unrated session, the outcome its commits or PRs point to, and why; yours to confirm. */
  suggestedOutcome: Outcome | null;
  evidence: string | null;
  title: string | null;
  firstPrompt: string | null;
  projectPath: string | null;
  repoRoot: string | null;
  gitBranch: string | null;
  model: string | null;
  startedAt: number | null;
  endedAt: number | null;
  activeMs: number;
  userTurns: number;
  assistantTurns: number;
  toolCalls: number;
  toolErrors: number;
  interruptions: number;
  permissionRequests: number;
  costUsd: number | null;
  costIsEstimate: boolean;
  premiumRequests: number | null;
  outcome: Outcome | null;
  rating: number | null;
  commits: number;
  insertions: number;
  deletions: number;
  aiCommits: number;
  waitMsTotal: number;
  waitCount: number;
  firstPromptLen: number;
  tokens: number;
  lastEvent: string | null;
  tags: string[];
  category: string | null;
  reads: number;
  edits: number;
  shellCalls: number;
  editsWithoutRead: number;
  reEdits: number;
  compactions: number;
  subagents: number;
  corrections: number;
  friction: Friction;
  hasSignals: boolean;
  vsManual: VsManual | null;
  reworked: boolean;
  agentMs: number;
  planned: boolean;
  peakContext: number;
  modeMs: Partial<Record<Mode, number>>;
  style: "one_shot" | "planned" | "iterative";
}

export type Mode = "coding" | "research" | "running" | "discussion";

export type VsManual = "faster" | "slower" | "same" | "unsure";

/** Heuristic 1–5 scores; 5 is the least friction. */
export interface Friction {
  requirements: number;
  steering: number;
  scope: number;
  overall: number;
}

export interface SessionDetail {
  session: SessionRow;
  filePath: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  linesAdded: number | null;
  linesRemoved: number | null;
  notes: string | null;
  toolCounts: Record<string, number>;
  toolErrorsBy: Record<string, number>;
  toolVetoes: Record<string, number>;
  /** [start, end, kind]: 1 = agent working, 0 = waiting on you. */
  lastMessage: string | null;
  resumeCommand: string | null;
  prompts: { idx: number; ts: number | null; text: string }[];
  commits: {
    sha: string;
    repoRoot: string;
    committedAt: number;
    subject: string;
    insertions: number;
    deletions: number;
    files: number;
    aiTrailer: boolean;
  }[];
  playbooks: {
    versionId: number;
    playbookId: number;
    name: string;
    kind: string;
    firstSeen: number;
    link: "auto" | "manual";
  }[];
  intent: { id: number; goal: string; expectedMinutes: number; confidence: number; outcome: Outcome | null } | null;
  team: Team;
  /** The lead's [start, end, kind] spans: 0 waiting on you, else a mode. */
  timeline: [number, number, number][];
}

/** "you" and "lead" are the two members every session has; agents use their own ids. */
export type AgentStatus = "running" | "done" | "failed" | "stopped";

export interface TeamAgent {
  id: string;
  name: string | null;
  /** Agent type: general-purpose, Explore, Task Agent, a custom agent… */
  role: string;
  task: string;
  brief: string | null;
  model: string | null;
  /** "lead" or the id of the agent that handed it the work. */
  parent: string;
  background: boolean;
  status: AgentStatus;
  startedAt: number | null;
  endedAt: number | null;
  toolCalls: number;
  tokens: number;
  tools: Record<string, number>;
  result: string | null;
}

export interface TeamMessage {
  ts: number | null;
  from: string;
  to: string;
  kind: "handoff" | "report" | "message";
  text: string;
}

export interface ScheduledWork {
  id: string | null;
  by: string;
  createdAt: number | null;
  cron: string | null;
  at: number | null;
  recurring: boolean;
  task: string;
  cancelled: boolean;
}

export interface Team {
  agents: TeamAgent[];
  messages: TeamMessage[];
  scheduled: ScheduledWork[];
  /** The session is still going. */
  live: boolean;
}

export interface Totals {
  sessions: number;
  activeHours: number;
  userTurns: number;
  toolCalls: number;
  toolErrors: number;
  interruptions: number;
  costUsd: number;
  costHasEstimates: boolean;
  premiumRequests: number;
  tokens: number;
  commits: number;
  aiCommits: number;
  rated: number;
  shipped: number;
  successRate: number | null;
  avgWaitSeconds: number | null;
  waitHours: number;
}

export interface GroupStat {
  key: string;
  sessions: number;
  activeHours: number;
  costUsd: number;
  commits: number;
  rated: number;
  successRate: number | null;
  avgTurns: number;
  avgInterruptions: number;
}

export interface Overview {
  totals: Totals;
  previous: Totals | null;
  bySource: GroupStat[];
  byProject: GroupStat[];
  byModel: GroupStat[];
  daily: { date: string; sessions: number; activeMinutes: number; costUsd: number; shipped: number; commits: number }[];
  outcomes: Record<string, number>;
  trend: Trend;
}

export interface Trend {
  weeks: string[];
  sessions: number[];
  activeHours: number[];
  successRate: (number | null)[];
  rated: number[];
  commits: number[];
  shipped: number[];
  waitHours: number[];
  interruptionsPerSession: (number | null)[];
  toolErrorRate: (number | null)[];
  costUsd: number[];
}

export interface Insight {
  id: string;
  kind: "positive" | "warning" | "info";
  title: string;
  detail: string;
  evidence: string;
  sessionIds: string[];
}

export interface Scorecard {
  sessions: number;
  sessionsInRepos: number;
  sessionsWithCommits: number;
  commitRate: number | null;
  commits: number;
  aiCommits: number;
  insertions: number;
  deletions: number;
  costPerCommit: number | null;
  activeMinutesPerCommit: number | null;
  costPerShipped: number | null;
  shipped: number;
  bySource: GroupStat[];
  byProject: GroupStat[];
  weekly: {
    weekStart: string;
    sessions: number;
    sessionsWithCommits: number;
    commits: number;
    shipped: number;
    activeHours: number;
  }[];
}



/**
 * Live sessions. The app pushes a new summary whenever it changes (the
 * "live" event, wired in App), so this fetches once when the window opens
 * and never polls.
 */
export const useLive = () => useQuery({ queryKey: ["live"], queryFn: () => api.live(), staleTime: Infinity });


export interface LiveAgent {
  id: string;
  name: string | null;
  role: string;
  task: string;
  status: LiveAgentStatus;
  startedAt: number | null;
  tokens: number;
}

export interface LiveSession {
  id: string;
  source: Source;
  /** Which app runs it: "cli", "claude-desktop", "copilot-app", "vscode"… */
  entrypoint: string | null;
  title: string | null;
  projectPath: string | null;
  gitBranch: string | null;
  status: LiveStatus;
  /** When it entered its current status. */
  statusSince: number;
  endedAt: number;
  userTurns: number;
  activeMs: number;
  lastMessage: string | null;
  pendingTool: string | null;
  outcome: Outcome | null;
  rating: number | null;
  mode: Mode | null;
  context: number;
  tokens: number;
  agents: LiveAgent[];
  scheduled: number;
  nextRun: number | null;
  /** Waiting on you for over 3 hours: listed apart, not counted as waiting now. */
  parked: boolean;
}

export interface LiveBucket {
  start: number;
  working: number;
  awaiting: number;
  done: number;
}

export interface LiveSummary {
  sessions: LiveSession[];
  completedToday: LiveSession[];
  working: number;
  awaiting: number;
  agentsRunning: number;
  replyMedianMs: number | null;
  replyP75Ms: number | null;
  replyN: number;
  agentMsToday: number;
  /** Agent time waiting on you today in waits up to 30 min; longer ones are awayMsToday. */
  waitMsToday: number;
  awayMsToday: number;
  /** Sessions waiting on you but parked. */
  parked: number;
  turnsToday: number;
  agentsDoneToday: number;
  agentsFailedToday: number;
  peakParallelToday: number;
  buckets: LiveBucket[];
  /** Your replies today: <1 min, 1–5, 5–15, 15–60, >60 min. */
  replyHistogram: [number, number, number, number, number];
  midnight: number;
}

export interface AlertAsset {
  id: string;
  kind: "sound" | "image";
}

export interface TeamSession {
  id: string;
  source: Source;
  title: string | null;
  projectPath: string | null;
  startedAt: number | null;
  endedAt: number;
  /** All tokens the lead used, for the agents' share. */
  tokens: number;
  team: Team;
}

export interface ToolkitOutcomes {
  rated: number;
  success: number | null;
  shipped: number;
  reworked: number;
}

export interface ToolkitItem {
  key: string;
  sessions: number;
  uses: number;
  sources: Source[];
  /** Sessions that used it, per week, oldest first. */
  weekly: number[];
  with: ToolkitOutcomes;
  without: ToolkitOutcomes;
}

export interface Toolkit {
  weeks: string[];
  sessions: number;
  agents: ToolkitItem[];
  tools: ToolkitItem[];
  mcp: ToolkitItem[];
  skills: ToolkitItem[];
}


export interface Intent {
  id: number;
  createdAt: number;
  goal: string;
  taskType: string;
  projectPath: string | null;
  tool: Source | null;
  expectedMinutes: number;
  confidence: number;
  sessionId: string | null;
  sessionTitle: string | null;
  sessionActiveMinutes: number | null;
  outcome: Outcome | null;
  actualMinutes: number | null;
  reflection: string | null;
  closedAt: number | null;
}

export interface CalibrationRow {
  key: string;
  n: number;
  timeRatio: number | null;
  successRate: number;
  avgConfidence: number;
}

export interface Calibration {
  closed: number;
  open: number;
  overall: CalibrationRow | null;
  byTaskType: CalibrationRow[];
  byConfidence: CalibrationRow[];
  byTool: CalibrationRow[];
}

export interface PlaybookVersion {
  id: number;
  firstSeen: number;
  lastSeen: number;
  gitSha: string | null;
  content: string;
  sessions: number;
  rated: number;
  successRate: number | null;
  avgTurns: number | null;
  avgInterruptions: number | null;
  toolErrorRate: number | null;
  commitsPerSession: number | null;
  reworkRate: number | null;
}

export interface Playbook {
  id: number;
  kind: "claude_md" | "agents_md" | "copilot_instructions" | "prompt";
  name: string;
  path: string | null;
  projectRoot: string | null;
  /** Most recently in effect first, so versions[0] is current. firstSeen can be
   *  older than a later version's when an edit restores earlier content. */
  versions: PlaybookVersion[];
}

export interface Retro {
  id: number;
  createdAt: number;
  periodStart: number;
  periodEnd: number;
  engine: "claude_cli" | "anthropic_api";
  model: string | null;
  contentMd: string;
  reflection: string | null;
  suggestions: Suggestion[];
}

export interface Suggestion {
  title: string;
  hypothesis: string | null;
  metric: string;
  category: string | null;
}

export interface Reality {
  shippedWithCommits: number;
  shippedReworked: number;
  reworkRate: number | null;
  byAnswer: {
    answer: VsManual | "unanswered";
    sessions: number;
    rated: number;
    successRate: number | null;
    avgActiveMinutes: number | null;
    withCommits: number;
    reworkRate: number | null;
  }[];
  timeRatio: number | null;
  timeRatioN: number;
  leverage: number | null;
  agentHours: number;
  waitHours: number;
}

export interface FitCell {
  category: string;
  source: Source;
  sessions: number;
  rated: number;
  successRate: number | null;
  reworkRate: number | null;
  verdict: "delegate" | "structure" | "yourself" | "unknown";
  ids: string[];
}

export interface ToolHealth {
  tool: string;
  calls: number;
  errors: number;
  errorRate: number | null;
  vetoes: number;
}

export type ModeSplit = Record<Mode, number>;

export interface Execution {
  /** Hours per mode. */
  modes: ModeSplit;
  bySource: { label: string; split: ModeSplit }[];
  weekly: { label: string; split: ModeSplit }[];
  styles: {
    style: "one_shot" | "planned" | "iterative";
    sessions: number;
    rated: number;
    successRate: number | null;
    reworkRate: number | null;
    avgActiveMinutes: number | null;
    avgPrompts: number | null;
    avgTokens: number | null;
    avgCorrections: number | null;
    ids: string[];
  }[];
  context: {
    smartZoneTokens: number;
    measured: number;
    overZone: string[];
    compacted: string[];
    successWithin: number | null;
    ratedWithin: number;
    successOver: number | null;
    ratedOver: number;
    buckets: [string, number][];
  };
}

export interface Anomaly {
  id: string;
  title: string | null;
  source: Source;
  startedAt: number | null;
  reasons: string[];
}

export interface FailureCluster {
  cause: string;
  advice: string;
  ids: string[];
}

export interface Lane {
  id: string;
  source: Source;
  title: string | null;
  projectPath: string | null;
  /** [start, end, kind]: 1 = agent working, 0 = waiting on you. */
  spans: [number, number, number][];
}

export interface MetricDef {
  key: string;
  label: string;
  higherIsBetter: boolean;
  unit: "rate" | "score" | "number";
}

export interface Experiment {
  id: number;
  createdAt: number;
  title: string;
  hypothesis: string | null;
  metric: MetricDef;
  category: string | null;
  source: Source | null;
  startAt: number;
  endAt: number;
  baselineValue: number | null;
  baselineN: number;
  currentValue: number | null;
  currentN: number;
  applied: number;
  eligible: number;
  reading: "too_early" | "better" | "worse" | "no_change";
  status: "active" | "kept" | "dropped";
  verdictNote: string | null;
}

export interface GoalStatus {
  id: number;
  metric: MetricDef;
  target: number;
  current: number | null;
  n: number;
  met: boolean | null;
  counter: MetricDef;
  counterCurrent: number | null;
  counterBaseline: number | null;
  counterSlipping: boolean;
}

export interface AskCheck {
  goal: boolean;
  doneCriteria: boolean;
  anchors: boolean;
  constraints: boolean;
  verification: boolean;
  score: number;
}

export interface PreflightAdvice {
  category: string;
  categorySuccess: number | null;
  categoryN: number;
  clearSuccess: number | null;
  clearN: number;
  vagueSuccess: number | null;
  vagueN: number;
  timeRatio: number | null;
  ask: AskCheck;
}

export interface SourceStatus {
  source: Source;
  label: string;
  paths: string[];
  found: boolean;
  enabled: boolean;
  sessions: number;
}

export interface Settings {
  values: Record<string, string>;
  detectedClaudePath: string | null;
  hasApiKey: boolean;
  keychainError: string | null;
  dataDir: string;
  dbPath: string;
  /** The app log: background errors and failed commands. */
  logPath: string;
  defaultApiModel: string;
}

/** A task in a workstream; see workstreams.rs. */
export interface WsTask {
  /** `<session id>:<task id>`. */
  key: string;
  sessionId: string;
  title: string;
  status: TaskStatus;
  kind: TaskKind;
  /** Why it is human work, or "Set by you". */
  reason: string | null;
  agent: string | null;
  /** Keys of unfinished tasks it waits for. */
  blockedBy: string[];
  updatedAt: number | null;
  /** A tracker item's page. */
  url: string | null;
  /** A tracker item assigned to you. */
  mine: boolean;
  /** The tracker item it sits under, and how deep (0 at the top). */
  parent: string | null;
  depth: number;
  /** Sessions that worked on this tracker item. */
  sessions: string[];
}

export interface Workstream {
  key: string;
  title: string;
  /** Listed first, and whatever its age. */
  starred: boolean;
  /** You made it, rather than Tilly grouping it. */
  custom: boolean;
  /** The epic it was imported from, and when it was last fetched. */
  sourceUrl: string | null;
  syncedAt: number | null;
  /** Why the last fetch from the tracker failed, until one succeeds. */
  syncError: string | null;
  /** The imported issue or work item is closed in its tracker. */
  closed: boolean;
  /** What was imported: "Issue", or a work item type such as "User Story". */
  sourceKind: string | null;
  /** "owner/repo" of an imported GitHub epic. */
  trackerRepo: string | null;
  repo: string | null;
  branch: string | null;
  lastActivity: number;
  sessions: {
    id: string;
    source: Source;
    entrypoint: string | null;
    title: string | null;
    status: LiveStatus;
    endedAt: number;
    activeMs: number;
    costUsd: number | null;
    costIsEstimate: boolean;
    tokens: number;
    commits: number;
    prs: string[];
    outcome: Outcome | null;
  }[];
  agents: { sessionId: string; id: string; name: string | null; role: string; active: boolean }[];
  tasks: WsTask[];
  updates: { at: number | null; sessionId: string; agent: string; taskKey: string | null; text: string }[];
  /** Progress counts: an imported epic's leaf items, else agents' tasks. */
  counts: { done: number; inProgress: number; needsYou: number; decisions: number; waiting: number; pending: number; agent: number; human: number };
}

/** A release you can move to; see updates.rs `list`. */
export interface ReleaseInfo {
  version: string;
  publishedAt: string | null;
  /** Release notes, in Markdown. */
  notes: string;
  url: string;
  relation: "newer" | "current" | "previous";
  /** Has signed update files, so it can be installed from the app. */
  installable: boolean;
}

/** Download progress while installing a release (the "update-progress" event). */
export interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

const DAY_MS = 24 * 3_600_000;

/**
 * The releases you can move to. Checks on launch and daily when automatic
 * checks are on (the default); otherwise only when refetched by hand.
 */
export const useReleases = (autoCheck: boolean) =>
  useQuery({ queryKey: ["releases"], queryFn: () => api.listReleases(), enabled: autoCheck, staleTime: DAY_MS, refetchInterval: autoCheck ? DAY_MS : false, retry: false });

export interface IngestReport {
  scanned: number;
  updated: number;
  failed: number;
  errors: string[];
  durationMs: number;
}

export interface SessionFilter {
  days: number;
  source?: Source;
  outcome?: Outcome | "unrated";
  search?: string;
  project?: string;
  category?: string;
  model?: string;
  /** Local date, YYYY-MM-DD. */
  date?: string;
  /** 0 = Monday. */
  weekday?: number;
  hour?: number;
  ids?: string[];
}

export interface Usage {
  daily: { date: string; cost: Record<string, number>; tokens: Record<string, number>; input: number; output: number; cacheRead: number; cacheWrite: number }[];
  byModel: {
    model: string;
    sessions: number;
    costUsd: number;
    tokens: number;
    rated: number;
    successRate: number | null;
    avgTurns: number;
    avgActiveMinutes: number;
    costPerSession: number;
    avgFriction: number;
  }[];
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  cacheHitRate: number | null;
  cacheSavingsUsd: number;
  monthToDateUsd: number;
  projectedMonthUsd: number;
  monthCumulative: [string, number][];
  daysInMonth: number;
  budgetUsd: number | null;
  costPerSession: number | null;
  sessions: number;
  shipped: number;
}

export interface Benchmark {
  label: string;
  sessions: number;
  successRate: number | null;
  avgTurns: number | null;
  correctionsPerSession: number | null;
  interruptionsPerSession: number | null;
  avgActiveMinutes: number | null;
  reEditRate: number | null;
  avgFriction: number | null;
}

export interface Patterns {
  calendar: { date: string; sessions: number; activeMinutes: number }[];
  heatmap: number[][];
  categories: {
    category: string;
    sessions: number;
    rated: number;
    successRate: number | null;
    avgActiveMinutes: number;
    avgTurns: number;
    avgFriction: number;
  }[];
  tools: { tool: string; count: number }[];
  reads: number;
  edits: number;
  shellCalls: number;
  otherToolCalls: number;
  qualityWeekly: {
    weekStart: string;
    sessions: number;
    successRate: number | null;
    readEditRatio: number | null;
    editsWithoutReadRate: number | null;
    reEditRate: number | null;
    correctionsPerSession: number;
    avgFriction: number;
  }[];
  friction: Friction | null;
  thenVsNow: [Benchmark, Benchmark] | null;
  longest: { id: string; source: Source; title: string | null; activeMs: number; userTurns: number; outcome: Outcome | null }[];
  withCompaction: Benchmark;
  withoutCompaction: Benchmark;
  withSubagents: Benchmark;
}

export const api = {
  overview: (days: number) => call<Overview>("get_overview", { days }),
  insights: (days: number) => call<Insight[]>("get_insights", { days }),
  scorecard: (days: number) => call<Scorecard>("get_scorecard", { days }),
  sessions: (filter: SessionFilter) => call<SessionRow[]>("list_sessions", { filter }),
  exportSessions: (filter: SessionFilter) => call<string>("export_sessions", { filter }),
  usage: (days: number) => call<Usage>("get_usage", { days }),
  reality: (days: number) => call<Reality>("get_reality", { days }),
  execution: (days: number) => call<Execution>("get_execution", { days }),
  anomalies: (days: number) => call<Anomaly[]>("get_anomalies", { days }),
  failureClusters: (days: number) => call<FailureCluster[]>("get_failure_clusters", { days }),
  timelines: (since: number) => call<Lane[]>("get_timelines", { since }),
  fit: (days: number) => call<FitCell[]>("get_fit", { days }),
  toolHealth: (days: number) => call<ToolHealth[]>("get_tool_health", { days }),
  patterns: (days: number) => call<Patterns>("get_patterns", { days }),
  session: (id: string) => call<SessionDetail>("get_session", { id }),
  saveReview: (review: { id: string; outcome: Outcome | null; rating: number | null; notes: string | null; tags: string[]; vsManual: VsManual | null }) =>
    call<void>("save_review", { review }),
  quickReview: (id: string, outcome: Outcome, vsManual: VsManual | null) => call<void>("quick_review", { id, outcome, vsManual }),
  metrics: () => call<MetricDef[]>("list_metrics"),
  experiments: () => call<Experiment[]>("list_experiments"),
  createExperiment: (input: { title: string; hypothesis: string | null; metric: string; category: string | null; source: Source | null; days: number; retroId: number | null }) =>
    call<number>("create_experiment", { input }),
  experimentVerdict: (id: number, decision: "keep" | "drop" | "extend", note: string | null) => call<void>("experiment_verdict", { id, decision, note }),
  deleteExperiment: (id: number) => call<void>("delete_experiment", { id }),
  setExperimentApplied: (sessionId: string, experimentId: number, applied: boolean) => call<void>("set_experiment_applied", { sessionId, experimentId, applied }),
  goals: () => call<GoalStatus[]>("list_goals"),
  createGoal: (input: { metric: string; target: number; counterMetric: string }) => call<number>("create_goal", { input }),
  retireGoal: (id: number) => call<void>("retire_goal", { id }),
  preflight: (text: string, source: Source | null) => call<PreflightAdvice>("preflight", { text, source }),
  live: () => call<LiveSummary>("get_live"),
  testNotification: () => call<void>("test_notification"),
  goToAgent: (id: string) => call<string>("go_to_agent", { id }),
  claudeHook: () => call<{ installed: boolean; settingsPath: string }>("claude_hook_status"),
  setClaudeHook: (on: boolean) => call<{ installed: boolean; settingsPath: string }>("set_claude_hook", { on }),
  progress: (days: number) => call<ProgressT>("get_progress", { days }),
  bulkReview: (ids: string[], outcome: Outcome) => call<number>("bulk_review", { ids, outcome }),
  dismissSession: (id: string) => call<void>("dismiss_session", { id }),
  attention: (days: number) => call<Attention>("get_attention", { days }),
  takeOpenRequest: () => call<string | null>("take_open_request"),
  since: (since: number) => call<LiveSession[]>("get_since", { since }),
  teams: () => call<TeamSession[]>("list_teams"),
  toolkit: (days: number, source: Source | null) => call<Toolkit>("get_toolkit", { days, source }),
  rescan: (force: boolean) => call<IngestReport>("rescan", { force }),
  sources: () => call<SourceStatus[]>("get_sources"),
  setSourceEnabled: (source: Source, enabled: boolean) => call<void>("set_source_enabled", { source, enabled }),
  projects: () => call<string[]>("list_projects"),

  intents: () => call<Intent[]>("list_intents"),
  createIntent: (input: {
    goal: string;
    taskType: string;
    projectPath: string | null;
    tool: Source | null;
    expectedMinutes: number;
    confidence: number;
    doneWhen?: string | null;
    context?: string | null;
  }) => call<number>("create_intent", { input }),
  closeIntent: (input: { id: number; outcome: Outcome; actualMinutes: number | null; reflection: string | null }) =>
    call<void>("close_intent", { input }),
  reopenIntent: (id: number) => call<void>("reopen_intent", { id }),
  deleteIntent: (id: number) => call<void>("delete_intent", { id }),
  linkIntent: (id: number, sessionId: string | null) => call<void>("link_intent", { id, sessionId }),
  calibration: (days: number) => call<Calibration>("get_calibration", { days }),

  playbooks: () => call<Playbook[]>("list_playbooks"),
  createPromptTemplate: (name: string, content: string) => call<number>("create_prompt_template", { name, content }),
  updatePromptTemplate: (id: number, name: string, content: string) =>
    call<void>("update_prompt_template", { id, name, content }),
  deletePlaybook: (id: number) => call<void>("delete_playbook", { id }),
  setSessionPlaybook: (sessionId: string, versionId: number, linked: boolean) =>
    call<void>("set_session_playbook", { sessionId, versionId, linked }),

  retros: () => call<Retro[]>("list_retros"),
  previewRetro: (days: number, reflection: string | null) => call<string>("preview_retro", { days, reflection }),
  generateRetro: (engine: string, days: number, reflection: string | null) => call<Retro>("generate_retro", { engine, days, reflection }),
  deleteRetro: (id: number) => call<void>("delete_retro", { id }),

  settings: () => call<Settings>("get_settings"),
  listReleases: () => call<ReleaseInfo[]>("list_releases"),
  /** Cards only: no task tree, just the latest update. */
  workstreams: (days: number) => call<Workstream[]>("list_workstreams", { days }),
  workstream: (key: string, days: number) => call<Workstream>("get_workstream", { key, days }),
  setTaskKind: (taskKey: string, kind: TaskKind | null) => call<void>("set_task_kind", { taskKey, kind }),
  createWorkstream: (title: string, sessions: string[]) => call<string>("create_workstream", { title, sessions }),
  renameWorkstream: (key: string, title: string) => call<void>("rename_workstream", { key, title }),
  starWorkstream: (ws: Pick<Workstream, "key" | "repo" | "branch">, starred: boolean) =>
    call<void>("star_workstream", { key: ws.key, starred, repo: ws.repo, branch: ws.branch }),
  addToWorkstream: (key: string, sessionIds: string[]) => call<void>("add_to_workstream", { key, sessionIds }),
  removeFromWorkstream: (key: string, sessionId: string) => call<void>("remove_from_workstream", { key, sessionId }),
  deleteWorkstream: (key: string) => call<void>("delete_workstream", { key }),
  importWorkstream: (url: string) => call<string>("import_workstream", { url }),
  syncWorkstream: (key: string) => call<void>("sync_workstream", { key }),
  progressReport: (key: string) => call<string>("progress_report", { key }),
  postProgress: (key: string, body: string) => call<string>("post_progress", { key, body }),
  labelItem: (taskKey: string, kind: TaskKind) => call<void>("label_item", { taskKey, kind }),
  openTrackerLink: (url: string) => call<void>("open_tracker_link", { url }),
  installRelease: (version: string) => call<void>("install_release", { version }),
  setSetting: (key: string, value: string) => call<void>("set_setting", { key, value }),
  importAlertAsset: (bytes: Uint8Array) => call<AlertAsset>("import_alert_asset", bytes),
  readAlertAsset: (id: string) => call<ArrayBuffer>("read_alert_asset", { id }),
  deleteAlertAsset: (id: string) => call<void>("delete_alert_asset", { id }),
  alertProfile: (defaults: boolean) => call<AlertProfile>("get_alert_profile", { default: defaults }),
  playAlertSound: (sound: NonNullable<SoundChoice>, volume: number) => call<void>("play_alert_sound", { sound, volume }),
  setApiKey: (key: string) => call<void>("set_api_key", { key }),
  clearApiKey: () => call<void>("clear_api_key"),
};

/** Where your attention goes; see attention.rs. */
export interface Attention {
  shortMs: number;
  awayMs: number;
  waits: number;
  awaySpells: number;
  medianReplyMs: number | null;
  byHour: { hour: number; shortMs: number; awayMs: number; waits: number }[];
  projects: { project: string; waitMs: number; waits: number }[];
  worstHour: number | null;
}

/** A group of sessions in "what pays off"; see progress.rs. */
export interface PayoffGroup {
  label: string;
  sessions: number;
  rated: number;
  success: number | null;
  avgCostUsd: number | null;
  costPerShipped: number | null;
}

/** Month-by-month progress and what pays off; see progress.rs. */
export interface ProgressT {
  months: { month: string; sessions: number; rated: number; success: number | null; costUsd: number; costPerShipped: number | null; replyMs: number | null; waitedMs: number; activeMs: number }[];
  headline: string | null;
  byTeam: PayoffGroup[];
  byApp: PayoffGroup[];
  byKind: PayoffGroup[];
}
