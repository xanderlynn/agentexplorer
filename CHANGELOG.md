# Changelog

All notable changes to Tilly are written here. Tilly shows this file in the app under **What's new**, and each GitHub release uses its section as the release notes.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow [Semantic Versioning](https://semver.org/). While the version starts with 0, anything may change between minor versions.

To add to the next release, write under **Unreleased**; `npm run release -- <version>` turns that section into the release's notes.

## [Unreleased]

### Added

- **Workstreams** under Agents: larger efforts across sessions as cards, with progress, the agents on each and their latest update. Open one for its task tree, built from the agents' own task lists (Claude Code and Copilot), with each task marked as agent work or yours.
- **Your own workstreams**: star a workstream to keep it at the top, make your own and add sessions to it, rename them, or import one from a GitHub issue or an Azure DevOps epic or feature, whose items become its tasks. Imported ones stay current in the background (every 15 minutes by default) and say why when they can't be fetched.
- **Updates and rollbacks** in Settings › Updates, and an **Update available** button in the sidebar. Pick any newer release, or the one before yours, and Tilly downloads it, checks its signature, installs it and restarts. Tilly checks GitHub for releases on launch and once a day; you can turn that off.
- A log file for background errors and failed actions: Settings › Data › **Show log**. Include it when you report a problem.

- **While you were away** on Today: what finished, failed or started waiting on you since you last looked.
- The tray menu lists the sessions waiting on you; pick one to open it.
- Sessions are labelled by the app that ran them: the **Claude app** and the **Copilot app** apart from their command-line tools.
- Agents' tasks can be **Blocked**, as Copilot reports them.
- Imported epics bring their whole tree: sub-issues of sub-issues on GitHub, and an Azure DevOps epic's features and their stories, three levels down, with progress counted on the leaves. Large epics are fetched in full, page by page.
- Each epic item shows the sessions that worked on it, who it's assigned to (and whether that's you), and turns In progress while an agent on it is working.
- Workstream cards say **Closed** for a closed epic, **Idle** after a week without activity, and when an imported epic last synced.

### Changed

- Waiting on you is harder to miss: a question asked overnight, or a permission prompt left through a meeting, stays in the Control center for a day instead of turning Done or Idle. Notifications say what the agent wants, remind you every 15 minutes (up to three times) while it waits, and open the session when clicked. "Might need approval" now notifies by default.
- "Waited on you" today counts the waits still open, not only the ones you've answered.
- Workstream counts are honest: an imported epic's progress is its own items, not those plus every agent todo; "need you" is split into tasks that are yours (assigned to you or nobody) and agents waiting on you; a todo carried into a later session counts once.
- Linking sessions to an epic is exact: "o/r#2" no longer matches "o/r#27", "#28" counts only in a clone of the epic's repository (by its git remote, not the folder name), and replies are searched as well as prompts. A session already in another workstream is linked to the item without being moved, and adding a session no longer takes a branch another workstream holds.
- Syncing an epic follows it: its new title (unless you renamed it), closed state, and new location if the issue moved. A failed sub-issue fetch is reported instead of silently emptying the epic, and a `gh` or `az` call that hangs gives up after a minute.
- Tasks that need credentials are recognised by what they ask ("Provide the staging password"), not by a word in them ("Fix password reset").
- Today no longer asks you to rate sessions that are still working or waiting on you.
- What's new no longer opens by itself after an update; a dot in the sidebar marks unread notes.
- A new logo: a girl with a bob haircut replaces the lowercase "t", in the app icon, the window and the macOS menu bar.

## [0.1.2] - 2026-10-28

The first public release of Tilly. Everything listed under 0.1.1 and 0.1.0 is included.

### Added

- **Portable downloads.** A ZIP for each platform that runs without installing: `Tilly.app` for macOS and `Tilly.exe` for Windows. Your data still lives in the usual app-data folder.
- **Build provenance.** Every download, the checksums and the SBOM carry a GitHub attestation, so you can check a file was built from this repository: `gh attestation verify <file> --repo xanderlynn/tilly`.

### Security

- Tilly's database, archived transcripts, alert sounds and images, retro working folder and CSV exports are now readable only by your user account.
- Reading git history no longer runs diff or text-conversion programs that a repository's own settings name.
- The app can only reveal files in Finder or Explorer; it can no longer open arbitrary links or programs.
- Updated dependencies, including OpenSSL (used only on Linux).

## [0.1.1] - 2026-10-28

Built but never published; its changes first ship in 0.1.2.

### Added

- **What's new.** These release notes in the app: they open once after each update, and the sidebar link shows your version.
- **Software bill of materials.** Each release lists every component that ships in Tilly, with its license, in `tilly-licenses.md` and in a CycloneDX SBOM (`tilly-sbom.cdx.json`).
- **Checksums.** Each release has a `SHA256SUMS.txt` for checking your download.

### Security

- Tilly runs `git` in the repositories your sessions worked in. It now stops a repository's own settings from making git run a program.
- Dependencies are checked against the known-vulnerability databases (RustSec and npm) on every change. None are known to be vulnerable.

## [0.1.0] - 2026-10-28

The first release of Tilly: a desktop app that keeps your AI coding agents in line and helps you get better at working with them. It reads Claude Code and GitHub Copilot logs (the CLI and Copilot Chat in VS Code) and keeps everything on your computer.

### Agents

- **Control center.** Every live session at a glance: what needs you, what is running and what finished today. An *Awaiting you* queue puts the longest wait first and shows what the agent asked. Working, scheduled and completed sessions sit in columns, each with its sub-agents.
- **Clear statuses.** Your turn, needs permission, might need approval, failed, working, long task, scheduled, completed and idle, each with its own colour, icon and label.
- **Live stats.** Sessions working and awaiting you, completed today, sub-agents, time spent waiting on you and your reply time, plus today's swimlanes and charts.
- **Teams.** See a session as a team: you, the lead agent and every sub-agent with its role, task, status and report, a flow of hand-offs and messages between them, and work scheduled to run later.
- **One session view.** Status, key numbers, team, timeline and your review on one page, with no tabs to flip between.

### Alerts

- **Your sounds and images per status.** Pick a built-in tone or your own WAV or Ogg file, an image, a volume and whether to send a desktop notification. Master volume, mute, quiet hours and background-only.
- **Runs in the tray.** Tilly keeps watching and alerting with its window closed. Open it from the menu bar or system tray; quit from the tray menu.

### Understand how you work

- **Sessions, outcomes and habits.** Every session from every tool, rated in one tap; what shipped and what was reworked; where agent time goes; and patterns over weeks.
- **Toolkit.** The agent roles, tools, MCP servers and skills you use, week by week, and how sessions that used each went.
- **Usage.** Tokens and spend, cache savings and a model comparison.
- **Improve.** A weekly retro, experiments with a locked baseline, and goals with a counter-metric.

### Under the hood

- Lean on resources: Tilly reads only the new lines of each log, pushes changes to the window instead of polling, and frees its window's memory when you close it. In the tray with agents busy, it uses under 1% of one CPU core and about 100 MB of memory.
- Upgrading from Governess or Execution Ledger: your data and saved API key move over on first launch.

[Unreleased]: https://github.com/xanderlynn/tilly/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/xanderlynn/tilly/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/xanderlynn/tilly/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/xanderlynn/tilly/releases/tag/v0.1.0
