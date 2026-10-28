// Writes public/third-party-notices.txt: the license text of every npm
// package and Rust crate that ships inside the app, with identical texts
// printed once. MIT, Apache-2.0, BSD and ISC all require these notices to be
// distributed with the binary. Uses only Node built-ins and `cargo metadata`.
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { cargoCrates, hostTarget, npmPackages, root } from "./deps.mjs";

const LICENSE_FILE = /^(licen[cs]e|copying|notice)/i;

function licenseTexts(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => LICENSE_FILE.test(f))
    .sort()
    .map((f) => readFileSync(join(dir, f), "utf8").trim());
}

/** @type {{ name: string; license: string; texts: string[] }[]} */
const entries = npmPackages().map((p) => ({ name: `${p.name}@${p.version} (npm)`, license: p.license ?? "unknown", texts: licenseTexts(p.dir) }));

// Release builds always have Rust; a frontend-only build may not, so the
// crates are skipped with a warning instead of failing.
try {
  for (const c of cargoCrates([hostTarget()])) {
    entries.push({ name: `${c.name}@${c.version} (crate)`, license: c.license ?? "unknown", texts: licenseTexts(c.dir) });
  }
} catch {
  console.warn("warning: cargo not found; third-party notices list npm packages only.");
}

// Group packages that ship byte-identical license texts.
const groups = new Map();
for (const e of entries.sort((a, b) => a.name.localeCompare(b.name))) {
  const key = e.texts.length ? e.texts.join("\n\n") : `(no license file; declared license: ${e.license})`;
  if (!groups.has(key)) groups.set(key, []);
  groups.get(key).push(`${e.name} — ${e.license}`);
}

const rule = "-".repeat(78);
let out = `Tilly includes the following third-party software.\n${entries.length} packages.\n\n`;
for (const [text, names] of groups) out += `${rule}\n${names.join("\n")}\n${rule}\n\n${text}\n\n`;
// public/ holds only generated files, so it may not exist in a fresh clone.
mkdirSync(join(root, "public"), { recursive: true });
writeFileSync(join(root, "public", "third-party-notices.txt"), out);
console.log(`Wrote third-party notices for ${entries.length} packages in ${groups.size} license groups.`);
