// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
// Field names mirror the Rust structs (serde camelCase).
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export type Source = "claude_code" | "copilot_cli" | "copilot_vscode";
export type Outcome = "shipped" | "partial" | "abandoned" | "redo";
export const OUTCOMES: Outcome[] = ["shipped", "partial", "abandoned", "redo"];

export interface SessionRow {
  id: string;
  source: Source;
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

export type LiveState = "working" | "quiet" | "needs_you" | "waiting" | "idle" | "ended";

/** The agent is stopped until you reply or approve something. */
export const isWaiting = (state: LiveState) => state === "waiting" || state === "needs_you";

/** Live sessions, polled every 5 s. Every caller shares the one poll. */
export const useLive = () =>
  useQuery({ queryKey: ["live"], queryFn: () => api.live(), refetchInterval: 5000, refetchIntervalInBackground: true, staleTime: 0 });

export interface LiveSession {
  id: string;
  source: Source;
  title: string | null;
  projectPath: string | null;
  gitBranch: string | null;
  state: LiveState;
  ageMs: number;
  userTurns: number;
  activeMs: number;
  avgWaitMs: number | null;
  lastMessage: string | null;
  outcome: Outcome | null;
  mode: Mode | null;
  context: number;
  /** Sub-agents still at work: [role or name, task]. */
  agentsRunning: [string, string][];
  agentsTotal: number;
  scheduled: number;
}

export interface TeamSession {
  id: string;
  source: Source;
  title: string | null;
  projectPath: string | null;
  startedAt: number | null;
  endedAt: number;
  state: LiveState;
  /** All tokens the lead used, for the agents' share. */
  tokens: number;
  team: Team;
}

export interface LiveSummary {
  sessions: LiveSession[];
  working: number;
  waitingOnYou: number;
  waitMsToday: number;
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
  defaultApiModel: string;
}

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
  overview: (days: number) => invoke<Overview>("get_overview", { days }),
  insights: (days: number) => invoke<Insight[]>("get_insights", { days }),
  scorecard: (days: number) => invoke<Scorecard>("get_scorecard", { days }),
  sessions: (filter: SessionFilter) => invoke<SessionRow[]>("list_sessions", { filter }),
  exportSessions: (filter: SessionFilter) => invoke<string>("export_sessions", { filter }),
  usage: (days: number) => invoke<Usage>("get_usage", { days }),
  reality: (days: number) => invoke<Reality>("get_reality", { days }),
  execution: (days: number) => invoke<Execution>("get_execution", { days }),
  anomalies: (days: number) => invoke<Anomaly[]>("get_anomalies", { days }),
  failureClusters: (days: number) => invoke<FailureCluster[]>("get_failure_clusters", { days }),
  timelines: (since: number) => invoke<Lane[]>("get_timelines", { since }),
  fit: (days: number) => invoke<FitCell[]>("get_fit", { days }),
  toolHealth: (days: number) => invoke<ToolHealth[]>("get_tool_health", { days }),
  patterns: (days: number) => invoke<Patterns>("get_patterns", { days }),
  session: (id: string) => invoke<SessionDetail>("get_session", { id }),
  saveReview: (review: { id: string; outcome: Outcome | null; rating: number | null; notes: string | null; tags: string[]; vsManual: VsManual | null }) =>
    invoke<void>("save_review", { review }),
  quickReview: (id: string, outcome: Outcome, vsManual: VsManual | null) => invoke<void>("quick_review", { id, outcome, vsManual }),
  metrics: () => invoke<MetricDef[]>("list_metrics"),
  experiments: () => invoke<Experiment[]>("list_experiments"),
  createExperiment: (input: { title: string; hypothesis: string | null; metric: string; category: string | null; source: Source | null; days: number; retroId: number | null }) =>
    invoke<number>("create_experiment", { input }),
  experimentVerdict: (id: number, decision: "keep" | "drop" | "extend", note: string | null) => invoke<void>("experiment_verdict", { id, decision, note }),
  deleteExperiment: (id: number) => invoke<void>("delete_experiment", { id }),
  setExperimentApplied: (sessionId: string, experimentId: number, applied: boolean) => invoke<void>("set_experiment_applied", { sessionId, experimentId, applied }),
  goals: () => invoke<GoalStatus[]>("list_goals"),
  createGoal: (input: { metric: string; target: number; counterMetric: string }) => invoke<number>("create_goal", { input }),
  retireGoal: (id: number) => invoke<void>("retire_goal", { id }),
  preflight: (text: string, source: Source | null) => invoke<PreflightAdvice>("preflight", { text, source }),
  live: () => invoke<LiveSummary>("get_live"),
  teams: () => invoke<TeamSession[]>("list_teams"),
  rescan: (force: boolean) => invoke<IngestReport>("rescan", { force }),
  sources: () => invoke<SourceStatus[]>("get_sources"),
  setSourceEnabled: (source: Source, enabled: boolean) => invoke<void>("set_source_enabled", { source, enabled }),
  projects: () => invoke<string[]>("list_projects"),

  intents: () => invoke<Intent[]>("list_intents"),
  createIntent: (input: {
    goal: string;
    taskType: string;
    projectPath: string | null;
    tool: Source | null;
    expectedMinutes: number;
    confidence: number;
    doneWhen?: string | null;
    context?: string | null;
  }) => invoke<number>("create_intent", { input }),
  closeIntent: (input: { id: number; outcome: Outcome; actualMinutes: number | null; reflection: string | null }) =>
    invoke<void>("close_intent", { input }),
  reopenIntent: (id: number) => invoke<void>("reopen_intent", { id }),
  deleteIntent: (id: number) => invoke<void>("delete_intent", { id }),
  linkIntent: (id: number, sessionId: string | null) => invoke<void>("link_intent", { id, sessionId }),
  calibration: (days: number) => invoke<Calibration>("get_calibration", { days }),

  playbooks: () => invoke<Playbook[]>("list_playbooks"),
  createPromptTemplate: (name: string, content: string) => invoke<number>("create_prompt_template", { name, content }),
  updatePromptTemplate: (id: number, name: string, content: string) =>
    invoke<void>("update_prompt_template", { id, name, content }),
  deletePlaybook: (id: number) => invoke<void>("delete_playbook", { id }),
  setSessionPlaybook: (sessionId: string, versionId: number, linked: boolean) =>
    invoke<void>("set_session_playbook", { sessionId, versionId, linked }),

  retros: () => invoke<Retro[]>("list_retros"),
  previewRetro: (days: number, reflection: string | null) => invoke<string>("preview_retro", { days, reflection }),
  generateRetro: (engine: string, days: number, reflection: string | null) => invoke<Retro>("generate_retro", { engine, days, reflection }),
  deleteRetro: (id: number) => invoke<void>("delete_retro", { id }),

  settings: () => invoke<Settings>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  setApiKey: (key: string) => invoke<void>("set_api_key", { key }),
  clearApiKey: () => invoke<void>("clear_api_key"),
};
