// Releases Tilly in two steps, because main only takes pull requests:
//
//   npm run release -- 0.2.0          bumps the version everywhere on a
//                                     release/v0.2.0 branch and opens a PR
//   npm run release -- 0.2.0 --tag    after that PR is merged: tags main
//                                     and pushes the tag
//
// Write the release's notes under "## [Unreleased]" in CHANGELOG.md first;
// step one moves them under the new version. The tag starts
// .github/workflows/release.yml, which builds the installers into a draft
// GitHub Release whose notes come from that section. Versions follow semver;
// 0.x means the app is still young and anything may change between minor
// versions.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { cut, readChangelog, writeChangelog } from "./changelog.mjs";

const [version, flag] = process.argv.slice(2);
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version ?? "") || (flag && flag !== "--tag")) {
  console.error("Usage: npm run release -- <semver, e.g. 0.2.0 or 0.2.0-beta.1> [--tag]");
  process.exit(1);
}
const run = (cmd, ...args) => execFileSync(cmd, args, { encoding: "utf8" }).trim();
const git = (...args) => run("git", ...args);
const fail = (message) => {
  console.error(message);
  process.exit(1);
};
const tag = `v${version}`;

git("fetch", "--quiet", "origin", "main");

if (flag === "--tag") {
  // Tag exactly what was merged, so the release builds main as it is.
  const shipped = JSON.parse(git("show", "origin/main:package.json")).version;
  if (shipped !== version) fail(`main is at ${shipped}, not ${version}. Merge the release PR first.`);
  git("tag", "-a", tag, "-m", `Tilly ${tag}`, "origin/main");
  git("push", "origin", tag);
  // The release-tag ruleset stops the workflow's token from creating a
  // release for a v* tag, so the draft is created here, as you; the
  // workflow uploads the builds into it.
  run("gh", "release", "create", tag, "--draft", "--verify-tag", "--title", `Tilly ${tag}`, "--notes", "Notes are added when every installer is built.");
  console.log(`Pushed ${tag}. The release workflow builds a draft release:\n  https://github.com/xanderlynn/tilly/actions/workflows/release.yml`);
  process.exit(0);
}

if (git("status", "--porcelain")) fail("Commit or stash your changes first.");

const json = (path, set) => {
  const data = JSON.parse(readFileSync(path, "utf8"));
  set(data);
  writeFileSync(path, JSON.stringify(data, null, 2) + "\n");
};
// The [package] version only: the first `version = ` line in the manifest.
const toml = (path) =>
  writeFileSync(path, readFileSync(path, "utf8").replace(/^version = ".*"$/m, `version = "${version}"`));

const branch = `release/${tag}`;
git("switch", "--quiet", "-c", branch, "origin/main");

const today = new Date().toISOString().slice(0, 10);
const notes = cut(readChangelog(), version, today, "xanderlynn/tilly");
writeChangelog(notes);
json("package.json", (p) => (p.version = version));
json("package-lock.json", (p) => {
  p.version = version;
  if (p.packages?.[""]) p.packages[""].version = version;
});
json("src-tauri/tauri.conf.json", (c) => (c.version = version));
toml("src-tauri/Cargo.toml");
toml("src-tauri/core/Cargo.toml");
// Keep Cargo.lock in step without touching any dependency.
execFileSync("cargo", ["update", "--workspace", "--offline"], { cwd: "src-tauri", stdio: "inherit" });

git("add", "CHANGELOG.md", "package.json", "package-lock.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "src-tauri/core/Cargo.toml", "src-tauri/Cargo.lock");
git("commit", "--quiet", "-m", `Release ${tag}`);
git("push", "--quiet", "-u", "origin", branch);
const url = run(
  "gh", "pr", "create", "--base", "main", "--head", branch, "--title", `Release ${tag}`,
  "--body", `Bumps the version to ${version} and moves the Unreleased notes under it.\n\nAfter merging: \`npm run release -- ${version} --tag\``,
);
console.log(`Opened ${url}\nOnce CI passes and it is merged:\n  npm run release -- ${version} --tag`);
