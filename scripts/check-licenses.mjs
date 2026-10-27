// Fails if any npm package that ships with the app (not dev tooling) has a
// license outside the permissive allow-list. Reads package-lock.json only,
// so it needs no extra dependencies.
import { readFileSync } from "node:fs";

const ALLOWED = new Set(["MIT", "ISC", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "0BSD", "Zlib", "Unlicense", "CC0-1.0", "BlueOak-1.0.0"]);

/** Accepts SPDX expressions whose every alternative set is allowed ("MIT OR Apache-2.0", "MIT AND ISC"). */
function allowed(expr) {
  if (!expr) return false;
  return expr
    .replace(/[()]/g, "")
    .split(/\s+OR\s+/)
    .some((alt) => alt.split(/\s+AND\s+/).every((id) => ALLOWED.has(id.trim())));
}

const lock = JSON.parse(readFileSync(new URL("../package-lock.json", import.meta.url)));
const bad = [];
let count = 0;
for (const [path, pkg] of Object.entries(lock.packages)) {
  if (!path || pkg.dev || pkg.devOptional) continue;
  count++;
  if (!allowed(pkg.license)) bad.push(`${path.replace(/^.*node_modules\//, "")}: ${pkg.license ?? "unknown"}`);
}
if (bad.length) {
  console.error(`License check failed for ${bad.length} runtime package(s):\n  ${bad.join("\n  ")}`);
  process.exit(1);
}
console.log(`License check passed: ${count} runtime npm packages, all permissive.`);
