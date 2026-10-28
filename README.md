<p align="center"><img src="public/tilly.svg" width="128" alt="Tilly logo: a white lowercase t with a bow on a pink-to-purple tile"></p>

# Tilly

*Keeps your agents in line, and you improving.*

A desktop app that tracks how you work with AI coding agents (Claude Code and GitHub Copilot) and helps you get better at it.

Plenty of tools already show tokens and cost. This one focuses on **execution**: did the work ship, what did it take, and which of your habits make the difference.

Built with Tauri 2, Rust and React. Runs on macOS and Windows. All data stays on your computer.

## What it does

The app is organised around a daily and a weekly loop, in seven areas (press ⌘K anywhere to jump to a page or find any prompt you've written):

| Area | What you get |
| --- | --- |
| **Agents** | The landing page. **Control center:** every live Claude Code and Copilot session by status (your turn, needs permission, might need approval, failed, working, long task, scheduled, completed, idle), an *Awaiting you* queue sorted by wait time, working / scheduled / completed-today columns with each session's sub-agents, live stats (working now, awaiting you, completed today, sub-agents, time waiting on you, your reply time), today's swimlanes, concurrency and reply-time charts. **Teams:** sessions that used sub-agents, roles and how often they report back. You choose a sound (built-in tones or your own WAV or Ogg) and an image for each status in Settings, with volume, mute, quiet hours and desktop notifications. |
| **Today** | Planning and review: a pointer to anything awaiting you, recent sessions to rate in one tap, running experiments, goals, planned tasks, unusual sessions. |
| **Sessions** | Every session from every tool, searchable across all prompts, filterable, exportable to CSV. Charts and insights elsewhere open here with the matching filter. The session view puts everything on one page: its live status and key numbers at the top; what the lead last said or is waiting on, the team (each sub-agent's status and task), one timeline of the lead's work and waits on you with each sub-agent below it, and folded detail (roster with briefs and reports, the hand-off flow, flow score and tool use, your prompts) on the left; your review, the resume command, commits and playbooks on the right. It works for Claude Code and Copilot (CLI and VS Code). |
| **Outcomes** | **Summary:** KPIs with 12-week sparklines, your usual range and sample size; insights that open the sessions behind them; why sessions go wrong, grouped by cause. **Delivery & reality:** sessions to commits, rework within 7 days of shipping, "felt faster" against evidence, your estimate ratio, agent-to-human time, and a delegation-fit matrix (task type × tool). |
| **Habits** | **Patterns:** activity calendar, weekday × hour heatmap, task categories, tool mix, tool reliability and vetoes, learning curve (with playbook changes marked), then vs now. **Execution:** where agent time goes (coding, research, running commands, discussion), how you run sessions (one-shot, planned, iterative) and how they compare, and how often sessions outgrow the model's smart zone (peak context, compaction). **Toolkit:** the agent roles, tools, MCP servers and skills you use with Claude Code and Copilot, week by week, and how sessions that used each one went compared with sessions that didn't (success and rework). **Journal:** plan tasks with the pre-flight card, close them with what happened, calibration. **Playbooks:** CLAUDE.md / AGENTS.md / prompt-template versions compared by outcome and rework. |
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

- Everything lives in a single SQLite file in the app data folder (`~/Library/Application Support/com.xanderlynn.tilly` on macOS, `%APPDATA%\com.xanderlynn.tilly` on Windows). Data from the app's earlier names, Governess and Execution Ledger, moves there on first launch, and so does a saved API key.
- Sensitive app-data directories and files are restricted to the owning user. Archived transcripts remain until you delete the app data; disabling archiving only stops new copies. CSV exports are separate files in Downloads and remain until you delete them. See [SECURITY.md](SECURITY.md#retention-and-deletion) for deletion details.
- Nothing leaves the machine unless you click **Generate retro**. That sends the period's metrics, session titles, opening prompts (truncated) and your notes to Claude. **Preview what's sent** shows the exact text first.
- The retro uses your installed `claude` CLI and your existing subscription by default. If you choose the Anthropic API engine instead, the key is stored in the OS keychain, never in the database.

## Install

Download the latest release from the [Releases page](https://github.com/xanderlynn/tilly/releases/latest):

| Platform | File |
| --- | --- |
| macOS 11 or later, Apple Silicon and Intel | `Tilly_<version>_universal.dmg`, or `Tilly_<version>_macos-universal-portable.zip` |
| Windows 10 and 11, x64 | `Tilly_<version>_x64-setup.exe` (or `…_x64_en-US.msi` for managed installs), or `Tilly_<version>_windows-x64-portable.zip` |

Each release lists SHA-256 checksums in `SHA256SUMS.txt`; check a download with `shasum -a 256 <file>` (macOS) or `Get-FileHash <file>` (Windows).

For a portable install, unzip the archive before running it. On macOS, move `Tilly.app` wherever you want and open it; keep the bundle intact. On Windows, run `Tilly.exe` from the extracted folder. The Windows portable build uses the system Microsoft Edge WebView2 runtime, which is included with current Windows 10 and 11 installations; if it has been removed, install the [Evergreen WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) first. Portable builds still store settings and the database in the normal per-user app-data directory described under **Privacy**; they do not keep user data beside the executable.

Until the downloads are code-signed, the OS warns on first launch. This applies to both installers and portable archives. On macOS, right-click Tilly and choose **Open**, then **Open** again. On Windows, choose **More info → Run anyway**. Extract the archive before bypassing the warning rather than running Tilly from inside the ZIP.

Tilly lives in the menu bar (macOS) or the system tray (Windows). Closing the window keeps it running so it can alert you; choose **Quit Tilly** from the tray menu to stop it.

## Develop

Prerequisites: Node 22+, Rust (stable), and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev
```

Checks:

```bash
npm run build                      # typecheck + bundle the frontend
cd src-tauri && cargo test --workspace         # Rust unit tests (app + core)
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings
```

To check the parsers against your own logs (read-only, into a throwaway database):

```bash
cd src-tauri && cargo test --release -p tilly-core real_logs -- --ignored --nocapture
```

### Layout

Tilly runs in the tray. It keeps collecting, tracking statuses and alerting (notifications and sounds) with its window closed; the window is created when you open it and freed when you close it, which is most of the app's memory. Start it with `--background` to go straight to the tray, as at login. The live state is pushed to the window when it changes; nothing polls.

```text
src/                       React UI (pages/, components/, api.ts = typed command wrappers)
src-tauri/
  core/src/                tilly-core: the engine, no UI dependency (shared with a future headless collector)
    parsers/               one parser per tool → a normalized ParsedSession, read incrementally
    sources.rs             where each tool's logs live, grouping files into sessions
    ingest.rs              change detection, parallel parsing (rayon), upserts, archiving
    watcher.rs             filesystem events, periodic scans, live state pushed to a Sink
    live.rs                each session's status and today's live stats
    status_alerts.rs       which status changes to announce, per your profile
    team.rs, toolkit.rs    sub-agents and their messages; agents, tools, MCP servers, skills
    analytics.rs, trends.rs, experiments.rs, journal.rs, playbooks.rs, git.rs
  src/                     the desktop app
    background.rs          tray, window lifecycle, and the Sink: window events, notifications, sounds
    sound.rs               alert tones (synthesised) and your WAV/Ogg files
    retro.rs               the weekly retro (claude CLI or Anthropic API)
    commands.rs            the Tauri command surface
```

## Releases and signing

Versions follow [semver](https://semver.org). `0.x` releases are early: anything may change between minor versions.

Release notes live in [CHANGELOG.md](CHANGELOG.md) ([Keep a Changelog](https://keepachangelog.com) format). Add to the **Unreleased** section as you go; the app shows the notes under **What's new**, opening them once after each update.

`main` only accepts pull requests that pass CI, so a release takes two steps:

```bash
npm run release -- 0.2.0          # on a release/v0.2.0 branch: moves Unreleased notes under 0.2.0, bumps every manifest, opens a PR
npm run release -- 0.2.0 --tag    # once that PR is merged: tags main as v0.2.0 and pushes the tag
```

The script refuses to release with no notes under Unreleased, and refuses to tag until `main` carries the new version. Only repository admins can create `v*` tags, and nobody can move or delete one. The tag starts `.github/workflows/release.yml`. It builds the macOS universal `.dmg` and portable `.app` ZIP, plus the Windows `-setup.exe`, `.msi`, and portable executable ZIP, into a **draft** GitHub Release. It then adds every download to `SHA256SUMS.txt`, records GitHub artifact attestations for the downloads, checksum, and SBOM, and uses the version's CHANGELOG section as the release notes. The macOS ZIP is created with `ditto` so bundle permissions, symlinks, and resource metadata are preserved. The build jobs run in the protected `release` environment, so each run waits until you approve it (the run's page, **Review deployments**). Check the draft, then publish it. A version with a suffix (`0.2.0-beta.1`) is published as a pre-release.

Every push to `main` also builds installers and portable archives as workflow artifacts (`.github/workflows/build.yml`), for trying a change before it is released.

**Signing.** Code signing and macOS notarization remain pending the required credentials. macOS signing and notarization turn on by themselves once these repository secrets exist: `APPLE_CERTIFICATE` (base64 .p12), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`. They need an Apple Developer account. For Windows, Azure Trusted Signing is the low-cost option; see the [Tauri signing guides](https://tauri.app/distribute/sign/macos/) for both.

## Logo

`public/tilly.svg` is the source for the app icons. After editing it, run `npx tauri icon public/tilly.svg` and delete the `android`, `ios` and `64x64.png` outputs, which the desktop builds don't use. The macOS menu-bar icon, `src-tauri/icons/tray-template@2x.png`, is the same mark in black on transparent (macOS recolours it for light and dark menu bars).

## License and dependencies

Tilly is MIT licensed, and the dependency tree is kept permissive and small:

- **Policy:** only permissive licenses (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode and similar). MPL-2.0 is allowed only for a few crates that come with Tauri and are used unmodified. `src-tauri/deny.toml` and `scripts/check-licenses.mjs` enforce this in CI, so a restrictive dependency fails the build.
- **Notices:** `npm run build` generates `third-party-notices.txt` with every shipped package's license text. It's bundled with the app and viewable under Settings → About and licenses.
- **Fewer dependencies:** Markdown is rendered by a small built-in component instead of a library, and HTTPS uses the operating system's TLS (macOS Security framework, Windows SChannel), not a bundled TLS stack.

```bash
npm run check:licenses
```
