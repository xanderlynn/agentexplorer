// Writes fictional Claude Code and Copilot CLI logs for screenshots and demos.
// Nothing here comes from a real session.
//
//   node scripts/demo-data.mjs logs <home>      six weeks of history, plus
//                                               live sessions as of now
//   node scripts/demo-data.mjs reviews <home>   rates the finished sessions,
//                                               once Tilly has read the logs
//
// Then run Tilly against that home, e.g. on macOS:
//   HOME=<home> src-tauri/target/release/tilly
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [cmd, home] = process.argv.slice(2);
if (!["logs", "reviews"].includes(cmd) || !home) {
  console.error("Usage: node scripts/demo-data.mjs logs|reviews <home>");
  process.exit(1);
}

// Seeded, so the same screenshots come out each time.
let seed = 7;
const rand = () => ((seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31);
const pick = (xs) => xs[Math.floor(rand() * xs.length)];
const between = (lo, hi) => lo + Math.floor(rand() * (hi - lo + 1));
let ids = 0;
const uuid = () => {
  const h = (n) => Array.from({ length: n }, () => Math.floor(rand() * 16).toString(16)).join("");
  return `${h(8)}-${h(4)}-4${h(3)}-a${h(3)}-${h(12)}`;
};
const nextId = (p) => `${p}${(++ids).toString(36)}`;
const iso = (t) => new Date(t).toISOString();
const NOW = Date.now();
const MIN = 60_000;
const DAY = 86_400_000;

const PROJECTS = {
  "storefront-web": "/Users/demo/code/storefront-web",
  "billing-api": "/Users/demo/code/billing-api",
  "mobile-app": "/Users/demo/code/mobile-app",
  "platform-infra": "/Users/demo/code/platform-infra",
  "docs-site": "/Users/demo/code/docs-site",
};
const TASKS = [
  ["storefront-web", "Add saved carts to checkout", "Let signed-in shoppers save a cart and come back to it. Store it server-side, keyed by account."],
  ["storefront-web", "Fix layout shift on product pages", "The hero image pushes the price down after load. Reserve its space and check CLS in Lighthouse."],
  ["storefront-web", "Dark mode for account settings", "Account settings ignore the dark theme. Use the theme tokens like the rest of the app."],
  ["billing-api", "Retry failed webhook deliveries", "Webhooks that get a 5xx should retry with backoff, up to 6 times, and show up in the admin log."],
  ["billing-api", "Prorate plan upgrades", "Upgrading mid-cycle should charge only the difference for the remaining days. Add tests for leap years."],
  ["billing-api", "Speed up the invoices query", "GET /invoices takes 2s for big accounts. Find out why and fix it without changing the response."],
  ["mobile-app", "Offline queue for orders", "Orders placed offline should queue and send when the connection comes back, without duplicates."],
  ["mobile-app", "Upgrade navigation library", "Move to the new navigation major version. Keep deep links working."],
  ["mobile-app", "Crash on photo upload", "The app crashes uploading HEIC photos on older phones. Reproduce it and fix it."],
  ["platform-infra", "Terraform for the staging cluster", "Write Terraform for a staging cluster that mirrors production at a quarter of the size."],
  ["platform-infra", "Alert on slow database queries", "Add an alert when p95 query time stays over 500 ms for 10 minutes."],
  ["docs-site", "API reference from OpenAPI", "Generate the API reference pages from the OpenAPI file in billing-api."],
  ["docs-site", "Search for the docs", "Add search to the docs site. Keep it static: no server."],
];
const MODELS = ["claude-opus-5-5", "claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];
const ROLES = [
  ["Explore", "Map the code involved", "Find every file that touches this and summarise how they fit together."],
  ["Plan", "Plan the change", "Propose a step-by-step plan, with the files each step changes."],
  ["code-reviewer", "Review the diff", "Review the change for bugs and missing tests."],
  ["general-purpose", "Write the tests", "Write tests for the new behaviour and run them."],
];
const SKILLS = ["frontend-design", "code-review", "tdd", "security-review"];
const MCP = ["mcp__github__create_pull_request", "mcp__linear__get_issue", "mcp__playwright__browser_navigate", "mcp__sentry__get_issue"];
const FILES = ["src/checkout/cart.ts", "src/api/invoices.go", "src/screens/Orders.tsx", "main.tf", "src/components/Price.tsx", "internal/webhooks/retry.go", "src/search/index.ts"];

const line = (o) => JSON.stringify(o) + "\n";
const write = (path, text) => {
  mkdirSync(join(path, ".."), { recursive: true });
  writeFileSync(path, text);
};

// ------------------------------------------------------------ Claude Code

function claudeSession({ project, title, prompt, start, steps, model, agents = [], skill, mcp, end = "done", pending, finalText, cron }) {
  const sid = uuid();
  const cwd = PROJECTS[project];
  const base = join(home, ".claude/projects", cwd.replaceAll("/", "-"));
  const branch = title.toLowerCase().replace(/[^a-z]+/g, "-").replace(/^-|-$/g, "").slice(0, 30);
  const ctx = { sessionId: sid, cwd, gitBranch: `feature/${branch}`, entrypoint: "cli" };
  let t = start;
  let out = line({ type: "user", timestamp: iso(t), ...ctx, origin: { kind: "human" }, message: { role: "user", content: prompt } });
  out += line({ type: "custom-title", customTitle: title, sessionId: sid });
  const assistant = (content, stop = "tool_use", extra = {}) =>
    line({ type: "assistant", timestamp: iso(t), ...ctx, ...extra, message: { id: nextId("msg"), model, role: "assistant", stop_reason: stop, content, usage: { input_tokens: between(20, 400), output_tokens: between(200, 2400), cache_read_input_tokens: between(20_000, 140_000), cache_creation_input_tokens: between(0, 9000) } } });
  const result = (id, text = "ok", isError = false) =>
    line({ type: "user", timestamp: iso(t), ...ctx, message: { role: "user", content: [{ type: "tool_result", tool_use_id: id, content: text, is_error: isError }] } });
  const tool = (name, input, res = "ok", err = false) => {
    const id = nextId("toolu_");
    t += between(4, 40) * 1000;
    out += assistant([{ type: "tool_use", id, name, input }]);
    t += between(1, 8) * 1000;
    out += result(id, res, err);
  };

  if (skill) tool("Skill", { skill });
  for (const [role, task, brief] of agents) {
    const id = nextId("toolu_");
    t += 20_000;
    out += assistant([{ type: "tool_use", id, name: "Agent", input: { subagent_type: role, description: task, prompt: brief } }]);
    const aid = uuid().replaceAll("-", "").slice(0, 17);
    const sub = join(base, sid, "subagents", `agent-${aid}`);
    let st = t + 2000;
    let s = "";
    const n = between(4, 12);
    for (let i = 0; i < n; i++) {
      st += between(5, 30) * 1000;
      s += line({ type: "assistant", timestamp: iso(st), ...ctx, isSidechain: true, agentId: aid, message: { id: nextId("msg"), model: "claude-sonnet-5-5", stop_reason: "tool_use", content: [{ type: "tool_use", id: nextId("toolu_"), name: pick(["Read", "Grep", "Glob", "Bash"]), input: { file_path: `${cwd}/${pick(FILES)}` } }], usage: { input_tokens: 40, output_tokens: 500, cache_read_input_tokens: 30_000 } } });
    }
    const running = end === "team";
    if (!running) {
      st += 5000;
      s += line({ type: "assistant", timestamp: iso(st), ...ctx, isSidechain: true, agentId: aid, message: { id: nextId("msg"), model: "claude-sonnet-5-5", stop_reason: "end_turn", content: [{ type: "text", text: `${task}: done.` }], usage: { input_tokens: 40, output_tokens: 300 } } });
    }
    write(`${sub}.jsonl`, s);
    write(`${sub}.meta.json`, JSON.stringify({ agentType: role, description: task, toolUseId: id, ...(running ? {} : { finished: true }) }));
    if (!running) {
      t = st + 2000;
      out += line({ type: "user", timestamp: iso(t), ...ctx, message: { role: "user", content: [{ type: "tool_result", tool_use_id: id, content: [{ type: "text", text: `${task}: found what was needed and reported back.` }] }] }, toolUseResult: { agentId: aid, status: "completed", totalTokens: between(20_000, 90_000), totalToolUseCount: n, totalDurationMs: st - start } });
    } else {
      t = st;
    }
  }
  for (let i = 0; i < steps; i++) {
    const name = pick(["Read", "Read", "Edit", "Edit", "Bash", "Grep", "Write"]);
    const err = name === "Bash" && rand() < 0.12;
    tool(name, name === "Bash" ? { command: pick(["npm test", "go test ./...", "terraform plan", "npm run lint"]) } : { file_path: `${cwd}/${pick(FILES)}` }, err ? "1 test failed" : "ok", err);
    if (mcp && i === Math.floor(steps / 2)) tool(mcp, {});
  }
  if (cron) tool("CronCreate", { cron: "0 2 * * *", prompt: cron, recurring: true });
  t += between(5, 20) * 1000;
  if (end === "done") {
    out += assistant([{ type: "text", text: finalText ?? `${title}: done. Tests pass.` }], "end_turn");
    out += line({ type: "cost-state", timestamp: iso(t), sessionId: sid, totalLinesAdded: between(20, 600), totalLinesRemoved: between(5, 250) });
  } else if (end === "failed") {
    out += assistant([{ type: "text", text: "I ran out of room before finishing this change." }], "max_tokens");
  } else if (end === "pending") {
    out += assistant([{ type: "tool_use", id: nextId("toolu_"), name: pending.name, input: pending.input }]);
  } else if (end === "team") {
    // The lead waits on its running sub-agents.
  }
  write(join(base, `${sid}.jsonl`), out);
  return { sid, end: t };
}

// ------------------------------------------------------------ Copilot CLI

function copilotSession({ project, title, prompt, start, steps, model = "gpt-5.5", end = "done", agent }) {
  const sid = uuid();
  const cwd = PROJECTS[project];
  const dir = join(home, ".copilot/session-state", sid);
  write(join(dir, "workspace.yaml"), `id: ${sid}\ncwd: ${cwd}\nbranch: main\nname: ${title}\n`);
  let t = start;
  const ev = (type, data) => line({ type, timestamp: iso(t), data });
  let out = ev("session.start", { context: { cwd, branch: "main" }, selectedModel: model });
  out += ev("user.message", { content: prompt });
  out += ev("assistant.turn_start", {});
  if (agent) {
    const call = nextId("call_");
    t += 10_000;
    out += ev("tool.execution_start", { toolCallId: call, toolName: "task", arguments: { agent_type: "explore", description: agent, prompt: agent } });
    out += ev("subagent.started", { toolCallId: call, agentDisplayName: "Explore" });
    t += 90_000;
    out += ev("subagent.completed", { toolCallId: call, model, totalTokens: 41_000, totalToolCalls: 9, durationMs: 90_000 });
    out += ev("tool.execution_complete", { toolCallId: call, success: true, result: { content: "Mapped the code." } });
  }
  for (let i = 0; i < steps; i++) {
    const call = nextId("call_");
    t += between(5, 40) * 1000;
    const tool = pick(["view", "str_replace_editor", "bash", "bash"]);
    out += ev("tool.execution_start", { toolCallId: call, toolName: tool, arguments: tool === "bash" ? { command: "npm test" } : { command: tool === "view" ? "view" : "str_replace", path: `${cwd}/${pick(FILES)}` } });
    out += ev("tool.execution_complete", { toolCallId: call, success: rand() > 0.08, result: { content: "ok" } });
  }
  t += 8000;
  if (end === "permission") {
    const call = nextId("call_");
    out += ev("tool.execution_start", { toolCallId: call, toolName: "bash", arguments: { command: "kubectl rollout restart deploy/api -n staging" } });
    out += ev("permission.requested", { permissionRequest: { toolCallId: call, kind: "shell" } });
  } else {
    out += ev("assistant.message", { content: `${title}: finished.`, outputTokens: between(800, 4000) });
    out += ev("assistant.turn_end", {});
    t += 60_000;
    out += ev("session.shutdown", { totalPremiumRequests: between(1, 6), modelMetrics: { [model]: { usage: { inputTokens: between(40_000, 300_000), outputTokens: between(4000, 30_000), cacheReadTokens: between(100_000, 900_000), cacheWriteTokens: 0 } } }, codeChanges: { linesAdded: between(10, 300), linesRemoved: between(2, 120) } });
  }
  write(join(dir, "events.jsonl"), out);
  return { sid, end: t };
}

// ------------------------------------------------------------ history

function logs() {
  const reviews = [];
  for (let day = 42; day >= 1; day--) {
    const n = day % 7 === 0 || day % 7 === 6 ? between(0, 1) : between(2, 5);
    for (let k = 0; k < n; k++) {
      const [project, title, prompt] = pick(TASKS);
      const start = NOW - day * DAY + between(9, 17) * 3600_000 + between(0, 59) * MIN;
      const planned = rand() < 0.45;
      const agents = planned ? [ROLES[0], ROLES[1], ...(rand() < 0.5 ? [ROLES[2]] : [])] : rand() < 0.25 ? [ROLES[0]] : [];
      const failed = rand() < 0.06;
      const copilot = rand() < 0.28;
      const s = copilot
        ? copilotSession({ project, title, prompt, start, steps: between(8, 40), agent: planned ? "Map the code involved" : undefined })
        : claudeSession({ project, title, prompt, start, steps: between(10, 60), model: pick(MODELS), agents, skill: rand() < 0.3 ? pick(SKILLS) : undefined, mcp: rand() < 0.35 ? pick(MCP) : undefined, end: failed ? "failed" : "done" });
      // Planning first and a reviewer ship more often: the pattern the demo shows.
      const good = (planned ? 0.75 : 0.45) + (agents.includes(ROLES[2]) ? 0.12 : 0) - (failed ? 0.5 : 0);
      if (rand() < 0.8) {
        const r = rand();
        const outcome = r < good ? "shipped" : r < good + 0.2 ? "partial" : r < good + 0.3 ? "redo" : "abandoned";
        reviews.push({ sid: s.sid, outcome, rating: outcome === "shipped" ? between(4, 5) : outcome === "partial" ? 3 : between(1, 2), vsManual: outcome === "shipped" ? pick(["faster", "faster", "same"]) : pick(["slower", "same", "unsure"]) });
      }
    }
  }
  // Earlier today, finished.
  for (const [i, ago] of [[0, 300], [3, 210], [6, 140], [11, 95]]) {
    const [project, title, prompt] = TASKS[i];
    claudeSession({ project, title, prompt, start: NOW - ago * MIN, steps: between(15, 30), model: "claude-opus-5-5", agents: [ROLES[0], ROLES[2]] });
  }

  // Live right now.
  claudeSession({ project: "storefront-web", title: "Apple Pay at checkout", prompt: "Add Apple Pay as a payment option at checkout, behind the payments feature flag.", start: NOW - 25 * MIN, steps: 18, model: "claude-opus-5-5", agents: [ROLES[0]], end: "pending", pending: { name: "AskUserQuestion", input: { questions: [{ question: "Should Apple Pay show for guests or only signed-in shoppers?" }] } } });
  claudeSession({ project: "billing-api", title: "Migrate to the new payments SDK", prompt: "Move billing-api to the v2 payments SDK. Split the work across a team: map, migrate, test.", start: NOW - 14 * MIN, steps: 6, model: "claude-opus-5-5", agents: [ROLES[0], ["general-purpose", "Migrate the webhook handlers", "Port internal/webhooks to the v2 SDK."], ROLES[3]], end: "team" });
  claudeSession({ project: "mobile-app", title: "Fix the flaky orders test", prompt: "OrdersScreen.test.tsx fails about one run in five on CI. Find out why and fix it.", start: NOW - 9 * MIN, steps: 12, model: "claude-sonnet-5-5", end: "pending", pending: { name: "Edit", input: { file_path: `${PROJECTS["mobile-app"]}/src/screens/Orders.test.tsx` } } });
  claudeSession({ project: "docs-site", title: "Search for the docs", prompt: "Add static search to the docs site.", start: NOW - 4 * MIN, steps: 5, model: "claude-haiku-5-5", end: "pending", pending: { name: "Bash", input: { command: "npm run build" } } });
  claudeSession({ project: "platform-infra", title: "Nightly dependency audit", prompt: "Set up a nightly audit of our dependencies and open an issue for anything vulnerable.", start: NOW - 40 * MIN, steps: 8, model: "claude-sonnet-5-5", cron: "Audit dependencies and open issues for vulnerable ones", finalText: "The nightly audit runs at 02:00." });
  claudeSession({ project: "billing-api", title: "Prorate plan upgrades", prompt: TASKS[4][2], start: NOW - 50 * MIN, steps: 40, model: "claude-opus-5-5", end: "failed" });
  copilotSession({ project: "platform-infra", title: "Restart the staging API", prompt: "Staging API pods are stuck on the old config. Restart them.", start: NOW - 6 * MIN, steps: 4, end: "permission" });

  writeFileSync(join(home, ".demo-reviews.json"), JSON.stringify(reviews));
  console.log(`Wrote fictional logs to ${home}`);
}

// ------------------------------------------------------------ reviews

function reviewsCmd() {
  const reviews = JSON.parse(readFileSync(join(home, ".demo-reviews.json"), "utf8"));
  const db = join(home, "Library/Application Support/com.xanderlynn.tilly/ledger.sqlite3");
  const q = (s) => `'${String(s).replaceAll("'", "''")}'`;
  const sql = reviews
    .map((r) => `UPDATE sessions SET outcome=${q(r.outcome)}, rating=${r.rating}, vs_manual=${q(r.vsManual)}, reviewed_at=${NOW} WHERE external_id=${q(r.sid)};`)
    .join("\n");
  execFileSync("sqlite3", [db], { input: `BEGIN;\n${sql}\nCOMMIT;\n` });
  console.log(`Rated ${reviews.length} sessions in ${db}`);
}

cmd === "logs" ? logs() : reviewsCmd();
