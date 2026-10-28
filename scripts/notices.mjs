// Writes public/third-party-notices.txt: the license text of every npm
// package and Rust crate that ships inside the app, with identical texts
// printed once. MIT, Apache-2.0, BSD and ISC all require these notices to be
// distributed with the binary. Uses only Node built-ins and `cargo metadata`.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const LICENSE_FILE = /^(licen[cs]e|copying|notice)/i;

function licenseTexts(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => LICENSE_FILE.test(f))
    .sort()
    .map((f) => readFileSync(join(dir, f), "utf8").trim());
}

/** @type {{ name: string; license: string; texts: string[] }[]} */
const entries = [];

const lock = JSON.parse(readFileSync(join(root, "package-lock.json"), "utf8"));
for (const [path, pkg] of Object.entries(lock.packages)) {
  if (!path || pkg.dev || pkg.devOptional) continue;
  const name = path.replace(/^.*node_modules\//, "");
  entries.push({ name: `${name}@${pkg.version} (npm)`, license: pkg.license ?? "unknown", texts: licenseTexts(join(root, path)) });
}

// Release builds always have Rust; a frontend-only build may not, so the
// crates are skipped with a warning instead of failing.
let rust = null;
try {
  const host = /host: (\S+)/.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))?.[1];
  rust = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--manifest-path", join(root, "src-tauri", "Cargo.toml"), ...(host ? ["--filter-platform", host] : [])], {
      encoding: "utf8",
      maxBuffer: 256 * 1024 * 1024,
    }),
  );
} catch {
  console.warn("warning: cargo not found; third-party notices list npm packages only.");
}
if (rust) {
  const used = new Set(rust.resolve.nodes.map((n) => n.id));
  const own = new Set(rust.workspace_members);
  for (const p of rust.packages) {
    if (!used.has(p.id) || own.has(p.id)) continue;
    entries.push({ name: `${p.name}@${p.version} (crate)`, license: p.license ?? "unknown", texts: licenseTexts(dirname(p.manifest_path)) });
  }
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
