import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  artifactName,
  createPortableArchive,
  packagePlan,
} from "./package-portable.mjs";

test("uses clear versioned portable artifact names", () => {
  assert.equal(
    artifactName("macos", "1.2.3"),
    "Tilly_1.2.3_macos-universal-portable.zip",
  );
  assert.equal(
    artifactName("windows", "1.2.3-beta.1"),
    "Tilly_1.2.3-beta.1_windows-x64-portable.zip",
  );
});

test("rejects unsupported platforms and unsafe versions", () => {
  assert.throws(() => artifactName("linux", "1.2.3"), /Unsupported platform/);
  assert.throws(() => artifactName("macos", "../1.2.3"), /Invalid version/);
});

test("macOS packaging keeps the app parent and bundle metadata", () => {
  const plan = packagePlan({ platform: "macos", version: "1.2.3", root: "/repo" });
  assert.equal(plan.executable, "ditto");
  assert.deepEqual(plan.args.slice(0, 4), [
    "-c",
    "-k",
    "--sequesterRsrc",
    "--keepParent",
  ]);
  assert.match(plan.source, /Tilly\.app$/);
  assert.match(plan.destination, /Tilly_1\.2\.3_macos-universal-portable\.zip$/);
});

test("Windows packaging uses literal paths and a controlled staging directory", () => {
  const plan = packagePlan({ platform: "windows", version: "1.2.3", root: "C:\\repo" });
  assert.equal(plan.executable, "powershell.exe");
  assert.match(plan.args.join(" "), /Get-ChildItem -LiteralPath/);
  assert.match(plan.args.join(" "), /Compress-Archive -LiteralPath/);
  assert.match(plan.stagedExecutable, /Tilly\.exe$/);
  assert.match(plan.destination, /Tilly_1\.2\.3_windows-x64-portable\.zip$/);
});

test(
  "creates a macOS archive that preserves executable permissions and symlinks",
  { skip: process.platform !== "darwin" },
  (t) => {
    const root = mkdtempSync(join(tmpdir(), "tilly-portable-"));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const app = join(
      root,
      "src-tauri/target/universal-apple-darwin/release/bundle/macos/Tilly.app",
    );
    const macos = join(app, "Contents/MacOS");
    const resources = join(app, "Contents/Resources");
    mkdirSync(macos, { recursive: true });
    mkdirSync(resources, { recursive: true });
    writeFileSync(join(macos, "Tilly"), "#!/bin/sh\n");
    chmodSync(join(macos, "Tilly"), 0o755);
    symlinkSync("../MacOS/Tilly", join(resources, "tilly-link"));

    const archive = createPortableArchive({
      platform: "macos",
      version: "1.2.3",
      root,
    });
    const extracted = join(root, "extracted");
    mkdirSync(extracted);
    execFileSync("ditto", ["-x", "-k", archive, extracted]);

    const executable = join(extracted, "Tilly.app/Contents/MacOS/Tilly");
    assert.equal(lstatSync(executable).mode & 0o111, 0o111);
    assert.equal(
      lstatSync(join(extracted, "Tilly.app/Contents/Resources/tilly-link")).isSymbolicLink(),
      true,
    );
  },
);

test(
  "creates a Windows archive containing the renamed executable",
  { skip: process.platform !== "win32" },
  (t) => {
    const root = mkdtempSync(join(tmpdir(), "tilly-portable-"));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const binary = join(root, "src-tauri/target/release/tilly.exe");
    mkdirSync(join(binary, ".."), { recursive: true });
    writeFileSync(binary, "fixture executable");

    const archive = createPortableArchive({
      platform: "windows",
      version: "1.2.3",
      root,
    });
    const extracted = join(root, "extracted");
    const command =
      "param([string]$Archive, [string]$Destination); Expand-Archive -LiteralPath $Archive -DestinationPath $Destination";
    execFileSync("powershell.exe", [
      "-NoLogo",
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      command,
      "-Archive",
      archive,
      "-Destination",
      extracted,
    ]);
    assert.equal(readFileSync(join(extracted, "Tilly.exe"), "utf8"), "fixture executable");
  },
);
