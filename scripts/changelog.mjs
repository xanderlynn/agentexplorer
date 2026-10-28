// Reads and updates CHANGELOG.md (Keep a Changelog format).
//
//   node scripts/changelog.mjs notes 0.2.0   prints that version's notes
//
// The release workflow uses `notes` for the GitHub release body;
// scripts/release.mjs uses `cut` to turn Unreleased into a version.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const FILE = new URL("../CHANGELOG.md", import.meta.url);
const HEADING = /^## \[([^\]]+)\].*$/gm;

/** The body under `## [name]`, without its heading. */
export function section(text, name) {
  const headings = [...text.matchAll(HEADING)];
  const i = headings.findIndex((m) => m[1] === name);
  if (i < 0) return null;
  const start = headings[i].index + headings[i][0].length;
  const end = headings[i + 1]?.index ?? text.search(/^\[[^\]]+\]: /m);
  return text.slice(start, end < 0 ? undefined : end).trim();
}

/** Moves the Unreleased notes under a new version heading. */
export function cut(text, version, date, repo) {
  const notes = section(text, "Unreleased");
  if (!notes) throw new Error("CHANGELOG.md has nothing under ## [Unreleased]. Write the release notes there first.");
  const prev = [...text.matchAll(HEADING)].map((m) => m[1]).find((v) => v !== "Unreleased");
  return text
    .replace(/^## \[Unreleased\][\s\S]*?(?=^## \[)/m, `## [Unreleased]\n\n## [${version}] - ${date}\n\n${notes}\n\n`)
    .replace(/^\[Unreleased\]: .*$/m, `[Unreleased]: https://github.com/${repo}/compare/v${version}...HEAD\n[${version}]: https://github.com/${repo}/${prev ? `compare/v${prev}...v${version}` : `releases/tag/v${version}`}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [cmd, name] = process.argv.slice(2);
  if (cmd !== "notes" || !name) {
    console.error("Usage: node scripts/changelog.mjs notes <version>");
    process.exit(1);
  }
  const notes = section(readFileSync(FILE, "utf8"), name);
  if (!notes) {
    console.error(`CHANGELOG.md has no section for ${name}`);
    process.exit(1);
  }
  console.log(notes);
}

export const changelogPath = FILE;
export const readChangelog = () => readFileSync(FILE, "utf8");
export const writeChangelog = (text) => writeFileSync(FILE, text);
