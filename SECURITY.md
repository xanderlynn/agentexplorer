# Security policy

## Supported versions

Security fixes go into the latest release. Update to it from the [Releases page](https://github.com/xanderlynn/tilly/releases/latest).

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub: **Security › Report a vulnerability** on this repository (a private security advisory). Don't open a public issue.

Include what you found, how to reproduce it, and the version you tested. You should hear back within a week.

## How Tilly handles your data

- Tilly reads AI-agent logs on your computer and stores what it learns in a local SQLite database. Nothing is sent anywhere unless you generate a weekly retro, which sends a summary to Claude; Settings shows exactly what is sent first.
- An Anthropic API key, if you use one, is kept in the operating system's keychain.
- Text from logs is treated as untrusted: it is shown as text, never as HTML, and the retro runs the `claude` CLI with no tools.
- Each release lists its components and their licenses in a CycloneDX SBOM (`tilly-sbom.cdx.json`), and CI checks dependencies against known-vulnerability databases.
