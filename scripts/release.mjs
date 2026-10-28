// Bumps Tilly's version everywhere it is written, commits, and tags it.
//
//   npm run release -- 0.2.0        then: git push && git push origin v0.2.0
//
// Write the release's notes under "## [Unreleased]" in CHANGELOG.md first;
// this moves them under the new version. The tag starts
// .github/workflows/release.yml, which builds the installers into a draft
// GitHub Release whose notes come from that section. Versions follow semver; 0.x means the app is
// still young and anything may change between minor versions.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { cut, readChangelog, writeChangelog } from "./changelog.mjs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version ?? "")) {
  console.error("Usage: npm run release -- <semver, e.g. 0.2.0 or 0.2.0-beta.1>");
  process.exit(1);
}
const git = (...args) => execFileSync("git", args, { encoding: "utf8" }).trim();
if (git("status", "--porcelain")) {
  console.error("Commit or stash your changes first.");
  process.exit(1);
}

const json = (path, set) => {
  const data = JSON.parse(readFileSync(path, "utf8"));
  set(data);
  writeFileSync(path, JSON.stringify(data, null, 2) + "\n");
};
// The [package] version only: the first `version = ` line in the manifest.
const toml = (path) =>
  writeFileSync(path, readFileSync(path, "utf8").replace(/^version = ".*"$/m, `version = "${version}"`));

const today = new Date().toISOString().slice(0, 10);
writeChangelog(cut(readChangelog(), version, today, "xanderlynn/tilly"));
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
git("commit", "-m", `Release v${version}`);
git("tag", "-a", `v${version}`, "-m", `Tilly v${version}`);
console.log(`Tagged v${version}. Push it to build the release:\n  git push && git push origin v${version}`);
