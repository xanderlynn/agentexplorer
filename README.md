# Execution Ledger

A desktop app that tracks how you work with AI coding agents (Claude Code and GitHub Copilot) and helps you get better at it.

Plenty of tools already show tokens and cost. This one focuses on **execution**: did the work ship, what did it take, and which of your habits make the difference.

Built with Tauri 2, Rust and React. Runs on macOS and Windows. All data stays on your computer.

## What it does

| Page | What you get |
| --- | --- |
| **Overview** | Sessions, success rate, commits, how long agents waited on you, spend, interruptions and tool errors, with trends against the previous period, plus rule-based insights about your habits. |
| **Live** | Every agent active in the last 3 hours, and whether it is working, waiting on you or asking for permission. Optional desktop notification when one starts waiting. |
| **Sessions** | Every session from every tool. Open one to rate the outcome (shipped / partial / abandoned / redo), add a 1–5 quality score and a note, and see its prompts, commits and the playbooks in effect. |
| **Journal** | Before you delegate, log the goal, expected time and confidence. Close it afterwards with the outcome and what surprised you. Calibration tables show whether you underestimate, and for which task types and tools. |
| **Playbooks** | Your CLAUDE.md, AGENTS.md and copilot-instructions.md files, with past versions read from git history, plus prompt templates you save. Each version shows the success rate, follow-up prompts and interruptions of the sessions that used it. |
| **Scorecard** | Output instead of activity: how many sessions lead to commits, cost and active time per commit, and weekly output. |
| **Weekly retro** | Claude reviews the period's data and your notes. You get what worked, where execution leaked, how you prompt, and three experiments for next week. It follows up on the previous retro's experiments. |

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

## License

MIT
