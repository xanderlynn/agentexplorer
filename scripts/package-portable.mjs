import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const VERSION = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;
const PLATFORMS = new Set(["macos", "windows"]);

export function artifactName(platform, version) {
  if (!PLATFORMS.has(platform)) throw new Error(`Unsupported platform: ${platform}`);
  if (!VERSION.test(version)) throw new Error(`Invalid version: ${version}`);
  const target = platform === "macos" ? "macos-universal" : "windows-x64";
  return `Tilly_${version}_${target}-portable.zip`;
}

export function packagePlan({ platform, version, root = process.cwd() }) {
  const destination = resolve(root, "artifacts", artifactName(platform, version));

  if (platform === "macos") {
    const source = resolve(
      root,
      "src-tauri/target/universal-apple-darwin/release/bundle/macos/Tilly.app",
    );
    return {
      source,
      destination,
      executable: "ditto",
      args: ["-c", "-k", "--sequesterRsrc", "--keepParent", source, destination],
    };
  }

  const source = resolve(root, "src-tauri/target/release/tilly.exe");
  const staging = resolve(root, "src-tauri/target/portable/windows-x64");
  const stagedExecutable = join(staging, "Tilly.exe");
  const script = resolve(root, "src-tauri/target/portable/package-portable.ps1");
  const scriptContent = [
    "param([string]$Source, [string]$Destination)",
    "$items = Get-ChildItem -LiteralPath $Source",
    "if (-not $items) { throw 'Portable staging directory is empty' }",
    "Compress-Archive -LiteralPath $items.FullName -DestinationPath $Destination -CompressionLevel Optimal -Force",
  ].join("\n");
  return {
    source,
    staging,
    stagedExecutable,
    script,
    scriptContent,
    destination,
    executable: "powershell.exe",
    args: [
      "-NoLogo",
      "-NoProfile",
      "-NonInteractive",
      "-File",
      script,
      "-Source",
      staging,
      "-Destination",
      destination,
    ],
  };
}

export function createPortableArchive(options) {
  const plan = packagePlan(options);
  const source = statSync(plan.source);
  if (options.platform === "macos" && !source.isDirectory()) {
    throw new Error(`Expected an app bundle at ${plan.source}`);
  }
  if (options.platform === "windows" && !source.isFile()) {
    throw new Error(`Expected an executable at ${plan.source}`);
  }

  mkdirSync(dirname(plan.destination), { recursive: true });
  rmSync(plan.destination, { force: true });

  if (options.platform === "windows") {
    rmSync(plan.staging, { recursive: true, force: true });
    mkdirSync(plan.staging, { recursive: true });
    copyFileSync(plan.source, plan.stagedExecutable);
    writeFileSync(plan.script, plan.scriptContent);
  }

  try {
    execFileSync(plan.executable, plan.args, { stdio: "inherit" });
    if (!statSync(plan.destination).isFile()) {
      throw new Error(`Portable archive was not created at ${plan.destination}`);
    }
    return plan.destination;
  } finally {
    if (options.platform === "windows") {
      rmSync(plan.staging, { recursive: true, force: true });
      rmSync(plan.script, { force: true });
    }
  }
}

function parseArgs(args) {
  const values = {};
  for (let i = 0; i < args.length; i += 2) {
    const name = args[i];
    const value = args[i + 1];
    if (!name?.startsWith("--") || value === undefined) {
      throw new Error("Usage: package-portable.mjs --platform <macos|windows> --version <semver>");
    }
    values[name.slice(2)] = value;
  }
  return values;
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) {
  try {
    const { platform, version } = parseArgs(process.argv.slice(2));
    const archive = createPortableArchive({ platform, version });
    console.log(`Created ${archive}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
