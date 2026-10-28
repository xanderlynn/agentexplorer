// The third-party packages that ship inside Tilly: npm runtime packages from
// package-lock.json and Rust crates from `cargo metadata`. Shared by the
// notices file (notices.mjs) and the SBOM (sbom.mjs). Node built-ins only.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** Platforms the release ships for; the SBOM lists crates for all of them. */
export const RELEASE_TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc"];

/** npm packages that ship in the app (no dev-only packages). */
export function npmPackages() {
  const lock = JSON.parse(readFileSync(join(root, "package-lock.json"), "utf8"));
  return Object.entries(lock.packages)
    .filter(([path, p]) => path && !p.dev && !p.devOptional)
    .map(([path, p]) => ({
      ecosystem: "npm",
      name: path.replace(/^.*node_modules\//, ""),
      version: p.version,
      license: p.license ?? null,
      dir: join(root, path),
      // "sha512-<base64>"
      integrity: p.integrity ?? null,
    }));
}

export function hostTarget() {
  return /host: (\S+)/.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))?.[1] ?? null;
}

/**
 * Rust crates linked into the app for each target (the app's own crates
 * excluded), with the targets each one is used on.
 */
export function cargoCrates(targets) {
  const lock = readFileSync(join(root, "src-tauri", "Cargo.lock"), "utf8");
  const checksums = new Map();
  for (const m of lock.matchAll(/name = "([^"]+)"\nversion = "([^"]+)"\nsource = "[^"]+"\nchecksum = "([0-9a-f]+)"/g)) {
    checksums.set(`${m[1]}@${m[2]}`, m[3]);
  }
  const crates = new Map();
  for (const target of targets) {
    const meta = JSON.parse(
      execFileSync(
        "cargo",
        ["metadata", "--format-version", "1", "--manifest-path", join(root, "src-tauri", "Cargo.toml"), "--filter-platform", target],
        { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
      ),
    );
    const used = new Set(meta.resolve.nodes.map((n) => n.id));
    const own = new Set(meta.workspace_members);
    for (const p of meta.packages) {
      if (!used.has(p.id) || own.has(p.id)) continue;
      const key = `${p.name}@${p.version}`;
      const c = crates.get(key) ?? {
        ecosystem: "cargo",
        name: p.name,
        version: p.version,
        license: p.license ?? (p.license_file ? `see ${p.license_file}` : null),
        dir: dirname(p.manifest_path),
        sha256: checksums.get(key) ?? null,
        repository: p.repository ?? null,
        targets: [],
      };
      c.targets.push(target);
      crates.set(key, c);
    }
  }
  return [...crates.values()];
}
