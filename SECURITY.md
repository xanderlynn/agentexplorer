# Security policy

## Supported versions

Security fixes go into the latest release. Update to it from the [Releases page](https://github.com/xanderlynn/tilly/releases/latest).

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub: **Security › Report a vulnerability** on this repository (a private security advisory). Don't open a public issue.

Include what you found, how to reproduce it, and the version you tested. You should hear back within a week.

## How Tilly handles your data

- Tilly reads AI-agent logs on your computer and stores what it learns in a local SQLite database. It runs no server and opens no listening port, has no telemetry or analytics, and loads no remote content: the window's content security policy allows only the app's own files.
- **Tokens.** Tilly stores no token for GitHub or Azure DevOps: it calls your installed `gh` and `az` CLIs, which use their own sign-in. An Anthropic API key, if you choose that retro engine, is kept in the operating system's keychain (Windows Credential Manager, macOS Keychain), never in the database or a file.
- **Secrets in your text.** Prompts, titles and agents' last replies are scrubbed before they're stored: known token shapes (GitHub, GitLab, Anthropic, OpenAI, Slack, npm, PyPI, Google, AWS), private key blocks, JWTs, bearer tokens and `password=`/`token:`-style values become `[redacted]`. Text stored by earlier versions is scrubbed once on upgrade, and the retro's text is scrubbed again before it's sent. Archived transcripts are exact copies of the tools' own logs and are not scrubbed (see below).
- The app-data directory, database (including SQLite sidecars), archived transcripts, imported alert assets, retro working directory, logs and CSV exports are created with access restricted to the owning user. On Unix this means `0700` directories and `0600` files; on Windows Tilly applies an owner-only protected access-control list.
- Text from logs is treated as untrusted: it is shown as text, never as HTML, links open only for GitHub and Azure DevOps items, and the retro runs the `claude` CLI with no tools.
- Each release lists its components and their licenses in a CycloneDX SBOM (`tilly-sbom.cdx.json`), publishes `SHA256SUMS.txt`, and carries GitHub build attestations. CI runs CodeQL and checks dependencies against known-vulnerability databases.

## For IT and security teams

**What leaves the machine, and how to turn it off**

| What | Where to | When | Turn off |
| --- | --- | --- | --- |
| Update check | `api.github.com` (public release list; no user data) | On launch and daily | Settings › Updates |
| Update download | `github.com` release assets, signature-verified | Only when you pick a version | Don't pick one |
| Imported workstreams | GitHub or Azure DevOps, through your `gh`/`az` sign-in | Every 15 minutes, for items you imported | Settings › Workstreams › Sync imported items: Off |
| Pull request state | GitHub, through `gh` (`gh pr view`) | Same loop, for PRs your sessions opened, last 30 days | Same setting |
| Share progress | A comment on your issue or work item | Only when you press Post | — |
| Weekly retro | Anthropic, through your `claude` CLI or API key | Only when you press Generate retro (Preview shows the exact text) | Don't generate one |

Nothing else makes a network connection. HTTPS uses the operating system's certificate store, so a corporate TLS-inspection root is trusted, and the system proxy settings (including Windows' proxy and PAC configuration) and `HTTPS_PROXY` are honoured.

**What it runs.** `git` (read-only: remotes and commits), `gh` and `az` (for the rows above), and `claude` (retro only). On macOS, Go to agent uses `open` and `osascript` to bring a Terminal or iTerm tab forward. Tilly needs no administrator rights; the portable Windows ZIP runs from any folder.

**What it changes outside its own folder.** Nothing, unless you turn on **Claude Code permission prompts**: that adds one Notification hook to `~/.claude/settings.json` (backed up first as `settings.json.tilly-backup`) which runs Tilly with `--hook` and records only the session and tool name. Turning it off removes the entry. Turn it off before deleting a portable copy, or Claude Code will report the missing program.

**Where data lives.** `%APPDATA%\com.xanderlynn.tilly` on Windows (`~/Library/Application Support/com.xanderlynn.tilly` on macOS), plus the system WebView's cache in `%LOCALAPPDATA%\com.xanderlynn.tilly`. The portable ZIP keeps no data next to the program.

**Code signing.** Windows builds are not yet Authenticode-signed, so SmartScreen warns on first run and an allow-list policy (AppLocker, WDAC) needs a hash or path rule. Verify a download against `SHA256SUMS.txt`, or its build provenance with `gh attestation verify <file> --repo xanderlynn/tilly`.

## Retention and deletion

- Every session's summary (times, cost, outcome, counts) is kept so trends cover your whole history. Its detail (prompts, timeline, team, task list, last reply) is removed after 180 days by default; Settings › Storage and history changes this (90, 180, 365 days, or forever).
- Transcript archiving is on by default because source tools may delete their logs (Claude Code after 30 days). Archived transcripts are compressed after a week and deleted after 90 days by default (30, 90, 365 days, or forever). Turning archiving off stops future copies; to remove existing ones, quit Tilly and delete the `archive` directory inside the app-data directory.
- Reviews, settings and retros remain until you remove Tilly's app-data directory.
- CSV exports are separate files in Downloads (or, if Downloads is unavailable, the app-data directory). Tilly does not automatically delete them.
- Removing Tilly does not remove its data. Quit the app before deleting the app-data directory.
