// Writes Tilly's software bill of materials: every third-party component
// that ships in the app, with its version, license, platforms and checksum.
//
//   node scripts/sbom.mjs [out-dir]     (default: dist-sbom/)
//
// Produces tilly-sbom.cdx.json (CycloneDX 1.5, readable by Dependency-Track,
// GitHub and most scanners) and tilly-licenses.md (a summary for people).
// The release workflow attaches both to every GitHub release.
import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { RELEASE_TARGETS, cargoCrates, npmPackages, root } from "./deps.mjs";

const out = process.argv[2] ?? join(root, "dist-sbom");
const version = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;

const purl = (c) =>
  c.ecosystem === "npm"
    ? `pkg:npm/${c.name.startsWith("@") ? "%40" + c.name.slice(1) : c.name}@${c.version}`
    : `pkg:cargo/${c.name}@${c.version}`;

// Older crates write "MIT/Apache-2.0"; SPDX says "MIT OR Apache-2.0".
const spdx = (l) => l.replace(/\s*\/\s*/g, " OR ");

// A single SPDX id goes in `license.id`; anything else as an expression.
const licenses = (raw) => {
  if (!raw) return [];
  if (raw.startsWith("see ")) return [{ license: { name: raw } }];
  const l = spdx(raw);
  return /^[A-Za-z0-9.+-]+$/.test(l) ? [{ license: { id: l } }] : [{ expression: l }];
};

const hashes = (c) => {
  if (c.sha256) return [{ alg: "SHA-256", content: c.sha256 }];
  const m = /^sha512-(.+)$/.exec(c.integrity ?? "");
  return m ? [{ alg: "SHA-512", content: Buffer.from(m[1], "base64").toString("hex") }] : [];
};

const components = [...npmPackages(), ...cargoCrates(RELEASE_TARGETS)]
  .sort((a, b) => a.ecosystem.localeCompare(b.ecosystem) || a.name.localeCompare(b.name) || a.version.localeCompare(b.version))
  .map((c) => ({
    type: "library",
    "bom-ref": purl(c),
    name: c.name,
    version: c.version,
    purl: purl(c),
    licenses: licenses(c.license),
    hashes: hashes(c),
    ...(c.repository ? { externalReferences: [{ type: "vcs", url: c.repository }] } : {}),
    properties: [
      { name: "tilly:ecosystem", value: c.ecosystem },
      ...(c.targets ? [{ name: "tilly:targets", value: c.targets.join(",") }] : []),
    ],
  }));

const bom = {
  bomFormat: "CycloneDX",
  specVersion: "1.5",
  serialNumber: `urn:uuid:${randomUUID()}`,
  version: 1,
  metadata: {
    timestamp: new Date().toISOString(),
    tools: { components: [{ type: "application", name: "tilly scripts/sbom.mjs" }] },
    component: {
      type: "application",
      "bom-ref": `pkg:github/xanderlynn/tilly@v${version}`,
      name: "Tilly",
      version,
      purl: `pkg:github/xanderlynn/tilly@v${version}`,
      licenses: [{ license: { id: "MIT" } }],
    },
  },
  components,
};

// The people-readable summary: components per license, then every component.
// "Apache-2.0 OR MIT" and "MIT OR Apache-2.0" are the same choice.
const canonical = (l) => (/\b(AND|WITH)\b|\(/.test(l) ? l : l.split(" OR ").sort().join(" OR "));
const licenseOf = (c) => canonical(c.licenses[0]?.license?.id ?? c.licenses[0]?.expression ?? c.licenses[0]?.license?.name ?? "unknown");
const byLicense = new Map();
for (const c of components) byLicense.set(licenseOf(c), (byLicense.get(licenseOf(c)) ?? 0) + 1);
const ecosystem = (c) => c.properties[0].value;
let md = `# Tilly ${version}: third-party components and licenses\n\n`;
md += `${components.length} components ship in Tilly ${version} (${components.filter((c) => ecosystem(c) === "npm").length} npm packages, ${components.filter((c) => ecosystem(c) === "cargo").length} Rust crates). Tilly itself is MIT licensed. The machine-readable SBOM is \`tilly-sbom.cdx.json\` (CycloneDX 1.5).\n\n`;
md += `## Licenses\n\n| License | Components |\n| --- | ---: |\n`;
for (const [l, n] of [...byLicense].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))) md += `| ${l} | ${n} |\n`;
md += `\n## Components\n\n| Component | Version | Ecosystem | License |\n| --- | --- | --- | --- |\n`;
for (const c of components) md += `| ${c.name} | ${c.version} | ${ecosystem(c)} | ${licenseOf(c)} |\n`;

mkdirSync(out, { recursive: true });
writeFileSync(join(out, "tilly-sbom.cdx.json"), JSON.stringify(bom, null, 2) + "\n");
writeFileSync(join(out, "tilly-licenses.md"), md);
console.log(`SBOM: ${components.length} components, ${byLicense.size} licenses → ${out}`);
