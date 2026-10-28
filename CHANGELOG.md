# Changelog

All notable changes to Tilly are written here. Tilly shows this file in the app under **What's new**, and each GitHub release uses its section as the release notes.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow [Semantic Versioning](https://semver.org/). While the version starts with 0, anything may change between minor versions.

To add to the next release, write under **Unreleased**; `npm run release -- <version>` turns that section into the release's notes.

## [Unreleased]

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
