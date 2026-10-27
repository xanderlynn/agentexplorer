# Execution Ledger

A desktop app that tracks how you work with AI coding agents (Claude Code and GitHub Copilot) and helps you get better at it.

Plenty of tools already show tokens and cost. This one focuses on **execution**: did the work ship, what did it take, and which of your habits make the difference.

Built with Tauri 2, Rust and React. Runs on macOS and Windows. All data stays on your computer.

## What it does

The app is organised around a daily and a weekly loop, in six areas (press ⌘K anywhere to jump to a page or find any prompt you've written):

| Area | What you get |
| --- | --- |
| **Today** | Agents waiting on you and what they asked; a swimlane of today's agents (working vs waiting on you); recent sessions to rate in one tap; running experiments; goals; planned tasks; unusual sessions. **Live agents** shows every active session's state. |
| **Sessions** | Every session from every tool, searchable across all prompts, filterable, exportable to CSV. Charts and insights elsewhere open here with the matching filter. The drawer shows the Flow score, tool use, failures and your rejections, the agent's last reply, commits (and whether they were reworked), and a resume command. |
| **Outcomes** | **Summary:** KPIs with 12-week sparklines, your usual range and sample size; insights that open the sessions behind them; why sessions go wrong, grouped by cause. **Delivery & reality:** sessions to commits, rework within 7 days of shipping, "felt faster" against evidence, your estimate ratio, agent-to-human time, and a delegation-fit matrix (task type × tool). |
| **Habits** | **Patterns:** activity calendar, weekday × hour heatmap, task categories, tool mix, tool reliability and vetoes, learning curve (with playbook changes marked), then vs now. **Execution:** where agent time goes (coding, research, running commands, discussion), how you run sessions (one-shot, planned, iterative) and how they compare, and how often sessions outgrow the model's smart zone (peak context, compaction). **Journal:** plan tasks with the pre-flight card, close them with what happened, calibration. **Playbooks:** CLAUDE.md / AGENTS.md / prompt-template versions compared by outcome and rework. |
| **Usage** | Tokens and spend side by side (tokens per session and per shipped session, a tokens / cost toggle), month-to-date spend against pace and an optional budget, cache hit rate and savings, and a model comparison. |
| **Improve** | **Weekly retro:** your reflection first, then Claude's review grounded in measured results, ending in three experiments you can adopt in one click. **Experiments & goals:** one habit per experiment with a locked baseline, adherence and a keep / drop / extend verdict; up to three goals, each with a counter-metric. |

Numbers are honest about their sample: rates show n, small samples read "k/n rated" instead of a percentage, and heuristic scores are labelled as such.

## Data sources

| Tool | Where it reads from |
| --- | --- |
| Claude Code (CLI, VS Code extension, desktop app) | `~/.claude/projects/**.jsonl` (or `$CLAUDE_CONFIG_DIR`) |
| GitHub Copilot CLI | `~/.copilot/session-state/*/events.jsonl` (or `$COPILOT_HOME`) |
| Copilot Chat in VS Code / Insiders / VSCodium | `<config>/Code/User/workspaceStorage/*/chatSessions/` |

The app watches these folders and updates within seconds. Claude Code deletes transcripts after 30 days by default, so the app keeps its own copy (configurable in Settings).

Copilot is a subscription, so the dollar figures for Copilot sessions are API-equivalent estimates for comparison. For Claude Code the app uses the cost Claude Code records itself.

Copilot inline completions are not logged to disk, so they can't be tracked.

## Privacy

- Everything lives in a single SQLite file in the app data folder (`~/Library/Application Support/com.xanderlynn.executionledger` on macOS, `%APPDATA%\com.xanderlynn.executionledger` on Windows).
- Nothing leaves the machine unless you click **Generate retro**. That sends the period's metrics, session titles, opening prompts (truncated) and your notes to Claude. **Preview what's sent** shows the exact text first.
- The retro uses your installed `claude` CLI and your existing subscription by default. If you choose the Anthropic API engine instead, the key is stored in the OS keychain, never in the database.

## Install

Download the latest installer from the **Build installers** workflow run's artifacts, or from a release:

- macOS: `Execution Ledger_<version>_universal.dmg`
- Windows: `Execution Ledger_<version>_x64_en-US.msi` or `…_x64-setup.exe`

The installers are not code-signed yet, so the OS warns on first launch. On macOS, right-click the app and choose **Open**. On Windows, choose **More info → Run anyway**.

## Develop

Prerequisites: Node 22+, Rust (stable), and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev
```

Checks:

```bash
npm run build                      # typecheck + bundle the frontend
cd src-tauri && cargo test         # Rust unit tests
cd src-tauri && cargo clippy --all-targets -- -D warnings
```

To check the parsers against your own logs (read-only, into a throwaway database):

```bash
cd src-tauri && cargo test --release real_logs -- --ignored --nocapture
```

### Layout

```text
src/                     React UI (pages/, components/, api.ts = typed command wrappers)
src-tauri/src/
  parsers/               one parser per tool → a normalized ParsedSession
  sources.rs             where each tool's logs live, grouping files into sessions
  ingest.rs              change detection, parallel parsing (rayon), upserts, archiving
  watcher.rs             filesystem events + periodic full scans
  git.rs                 links sessions to the commits they produced
  playbooks.rs           instruction-file and prompt-template versions
  journal.rs             intents, outcomes, calibration
  analytics.rs           overview, scorecard, insights
  live.rs                live session states
  retro.rs               the weekly retro (claude CLI or Anthropic API)
  commands.rs            the Tauri command surface
```

## Releases and signing

`.github/workflows/build.yml` builds a universal macOS `.dmg` and Windows `.msi`/`.exe` on every push to `main`; they appear as workflow artifacts. Pushing a tag such as `v0.2.0` also attaches them to a draft GitHub Release.

To sign, add the Apple (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`) and/or Windows signing secrets, then pass them to the `tauri-action` step as environment variables. See the [Tauri signing guides](https://tauri.app/distribute/sign/macos/).

## License and dependencies

Execution Ledger is MIT licensed, and the dependency tree is kept permissive and small:

- **Policy:** only permissive licenses (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode and similar). MPL-2.0 is allowed only for a few crates that come with Tauri and are used unmodified. `src-tauri/deny.toml` and `scripts/check-licenses.mjs` enforce this in CI, so a restrictive dependency fails the build.
- **Notices:** `npm run build` generates `third-party-notices.txt` with every shipped package's license text. It's bundled with the app and viewable under Settings → About and licenses.
- **Fewer dependencies:** Markdown is rendered by a small built-in component instead of a library, and HTTPS uses the operating system's TLS (macOS Security framework, Windows SChannel), not a bundled TLS stack.

```bash
npm run check:licenses
```
