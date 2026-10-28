# Security policy

## Supported versions

Security fixes go into the latest release. Update to it from the [Releases page](https://github.com/xanderlynn/tilly/releases/latest).

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub: **Security › Report a vulnerability** on this repository (a private security advisory). Don't open a public issue.

Include what you found, how to reproduce it, and the version you tested. You should hear back within a week.

## How Tilly handles your data

- Tilly reads AI-agent logs on your computer and stores what it learns in a local SQLite database. Nothing is sent anywhere unless you generate a weekly retro, which sends a summary to Claude; Settings shows exactly what is sent first.
- The app-data directory, database (including SQLite sidecars), archived transcripts, imported alert assets, retro working directory, and CSV exports are created with access restricted to the owning user. On Unix this means `0700` directories and `0600` files; on Windows Tilly applies an owner-only protected access-control list.
- An Anthropic API key, if you use one, is kept in the operating system's keychain.
- Text from logs is treated as untrusted: it is shown as text, never as HTML, and the retro runs the `claude` CLI with no tools.
- Each release lists its components and their licenses in a CycloneDX SBOM (`tilly-sbom.cdx.json`), and CI checks dependencies against known-vulnerability databases.

## Retention and deletion

- Parsed session data, reviews, settings, and retros remain in the SQLite database until you remove Tilly's app-data directory.
- Transcript archiving is enabled by default because source tools may delete their logs. Turning it off in Settings stops future copies; it does not delete existing copies. To remove them, quit Tilly and delete the `archive` directory inside the app-data directory.
- CSV exports are separate files in Downloads (or, if Downloads is unavailable, the app-data directory). Tilly does not automatically delete them.
- Removing Tilly does not remove its data. Quit the app before deleting the app-data directory: `~/Library/Application Support/com.xanderlynn.tilly` on macOS or `%APPDATA%\com.xanderlynn.tilly` on Windows.
