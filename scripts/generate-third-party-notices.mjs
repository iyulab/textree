/*
 * Regenerates THIRD-PARTY-NOTICES.md from the dependency graphs that actually ship.
 *
 * A hand-written inventory of several hundred dependencies is out of date the moment anything is
 * added, so the file is generated. Run this whenever dependencies change:
 *
 *   node scripts/generate-third-party-notices.mjs
 *
 * Everything that ends up inside the installed application feeds it:
 *   - Rust: the normal-dependency closure of the binary, resolved for the release target. Build-
 *     and dev-dependencies are excluded; they run during the build and are not distributed.
 *   - npm: the production-dependency closure, whose code is bundled into the web assets.
 *   - The publishing renderer, a bundled helper: the Node.js runtime it runs on and the npm
 *     closure installed from canopy-sidecar/package-lock.json.
 *   - The local AI helper, a bundled self-contained .NET program: the .NET runtime and every NuGet
 *     package that contributes runtime or native assets, read from the host's restored graph (so
 *     `dotnet restore src-host/TextreeHost.slnx` has to have run) and the packages' own nuspecs.
 *
 * Beside the table it writes THIRD-PARTY-LICENSES.txt, the license texts themselves: most of these
 * licenses make keeping the text, copyright line included, the condition of redistribution, so a
 * name in a table alone does not meet them. The texts come from the packages as installed — the
 * crates' sources, node_modules, the assembled renderer and the NuGet package folder — so the
 * renderer has to be assembled from its pin first. A package that publishes without its text is
 * named in scripts/notices-missing-texts.json, with why; the list may only shrink.
 *
 * The prelude is kept by hand: the native libraries compiled into the git engine are not visible
 * to any package manager, and their terms constrain how the whole distribution may be shipped.
 */

import { readFileSync } from "node:fs";
import { basename, join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  PERMISSIVE,
  applyPinned,
  cargoPackages,
  needsReview as unaccepted,
  noticesTable as table,
  noticesText,
  npmPackages,
  nugetPackages,
  withoutText,
  writeOrCheck,
} from "@iyulab/tauri-kit-dev/notices";
import { CANOPY_STAGE } from "./canopy-stage.mjs";
import { checkCanopy, describeCanopy } from "./sidecar-provenance.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const TARGET = "x86_64-pc-windows-msvc";
const OUTPUT = join(ROOT, "THIRD-PARTY-NOTICES.md");
const LICENSES_OUTPUT = join(ROOT, "THIRD-PARTY-LICENSES.txt");
const MISSING_TEXTS = join(ROOT, "scripts", "notices-missing-texts.json");
// License texts of packages that publish without one, taken from their source at the published
// version and checked against a SHA-256. Refresh with
// `npx tauri-kit-dev notice-pins --pins scripts/notice-pins.json --dir scripts/notice-texts`.
const PINS = join(ROOT, "scripts", "notice-pins.json");
const PINNED_TEXTS = join(ROOT, "scripts", "notice-texts");
const PRELUDE = join(ROOT, "scripts", "third-party-notices-prelude.md");
const RENDERER_LOCK = join(ROOT, "canopy-sidecar", "package-lock.json");
const RENDERER_ASSEMBLY = join(ROOT, "scripts", "assemble-canopy-sidecar.ps1");
const HOST_ASSETS = join(ROOT, "src-host", "src", "Textree.Host", "obj", "project.assets.json");
const HOST_PROPS = join(ROOT, "src-host", "Directory.Build.props");

/**
 * License expressions that are compatible with this application's GPL-3.0-only license: the
 * permissive ones, plus the copyleft terms and exceptions GPL-3.0-only can carry. Anything outside
 * this set is reported rather than silently listed: a copyleft term we cannot satisfy is a
 * licensing defect, not a formatting one.
 */
const COMPATIBLE = new Set([
  ...PERMISSIVE,
  "MPL-2.0", "GPL-3.0-only", "GPL-3.0-or-later", "LGPL-2.1-or-later", "LGPL-3.0-or-later",
]);

/** The Node.js version the renderer is bundled with, as the assembly script pins it. */
function rendererNodeVersion() {
  const m = readFileSync(RENDERER_ASSEMBLY, "utf8").match(/\$NodeVersion\s*=\s*'([0-9.]+)'/);
  if (!m) throw new Error("could not read the bundled Node.js version from scripts/assemble-canopy-sidecar.ps1");
  return m[1];
}

/** The .NET major the host is built for (net10.0 → 10). */
function hostDotnetMajor() {
  const m = readFileSync(HOST_PROPS, "utf8").match(/<TargetFramework>net(\d+)\.\d+<\/TargetFramework>/);
  if (!m) throw new Error("could not read the host's target framework from src-host/Directory.Build.props");
  return m[1];
}

const rust = cargoPackages({ cwd: join(ROOT, "src-tauri"), target: TARGET });
const npm = npmPackages({ lock: join(ROOT, "package-lock.json"), installedAt: ROOT });
// The renderer's closure comes from its committed lock file; its license texts from the assembled
// renderer, which therefore has to be the pinned release — an older stage would hand over another
// version's texts.
const canopy = checkCanopy();
if (canopy.verdict !== "pinned") {
  throw new Error(
    `the renderer's license texts are read from its assembled packages, and ${describeCanopy(canopy)} — ` +
      "run `pwsh scripts/assemble-canopy-sidecar.ps1` first",
  );
}
const renderer = npmPackages({ lock: RENDERER_LOCK, installedAt: CANOPY_STAGE });
const node = rendererNodeVersion();
const host = (() => {
  try {
    return nugetPackages({ assets: HOST_ASSETS });
  } catch {
    throw new Error("the host's dependency graph is missing — run `dotnet restore src-host/TextreeHost.slnx` first");
  }
})();
const dotnet = hostDotnetMajor();

const needsReview = unaccepted([...rust, ...npm, ...renderer, ...host], COMPATIBLE);
if (needsReview.length) {
  console.error("License expressions outside the known-compatible set — review before shipping:");
  for (const e of needsReview) console.error(`  ${e.name} ${e.version}: ${e.license}`);
}

const body = [
  readFileSync(PRELUDE, "utf8").trimEnd(),
  "",
  "## Rust crates",
  "",
  `The ${rust.length} crates below are the normal-dependency closure of the binary, resolved for`,
  "`" + TARGET + "`. Build-time and test-only dependencies are excluded because their code is not",
  "distributed.",
  "",
  table(rust),
  "",
  "## Bundled web dependencies",
  "",
  `The ${npm.length} packages below are the production-dependency closure of the web assets, which`,
  "are compiled into the application.",
  "",
  table(npm),
  "",
  "## Publishing renderer",
  "",
  "The renderer that turns notes into a website runs as a separate helper process on a bundled",
  "Node.js runtime.",
  "",
  `- **Node.js ${node}** — MIT — https://github.com/nodejs/node. The runtime itself contains`,
  "  components under their own licenses (V8, libuv, OpenSSL, ICU and others), reproduced in its",
  `  license file: https://github.com/nodejs/node/blob/v${node}/LICENSE`,
  "",
  `The ${renderer.length} packages below are the renderer's production-dependency closure.`,
  "",
  table(renderer),
  "",
  "## Local AI helper",
  "",
  "The helper that indexes notes and runs local models is a self-contained .NET program.",
  "",
  `- **.NET ${dotnet} runtime** — MIT — https://github.com/dotnet/runtime. Its own third-party`,
  "  notices: https://github.com/dotnet/runtime/blob/main/THIRD-PARTY-NOTICES.TXT",
  "",
  `The ${host.length} NuGet packages below put code into the helper (runtime or native assets).`,
  "",
  table(host),
  "",
  "The license texts these components carry are reproduced in THIRD-PARTY-LICENSES.txt, next to this",
  "file. For a component that publishes without one, the text is taken from its source at the same",
  "version and its origin is named there; the few whose source has none either are listed by name.",
  "",
].join("\n");

function summary() {
  return `${rust.length} crates, ${npm.length} web packages, renderer ${renderer.length} packages, AI helper ${host.length} packages`;
}

/**
 * The packages that publish without their license text, against the committed list of them. The
 * list may only shrink: a package missing from it is a new gap to close (ask its publisher to ship
 * the text), and an entry that no longer matches anything — the package now carries its text, or
 * is gone — has to be removed, so the list keeps meaning what it says.
 */
function missingTexts(packages) {
  const listed = JSON.parse(readFileSync(MISSING_TEXTS, "utf8"));
  const missing = new Set(withoutText(packages).map((p) => p.name));
  return {
    unlisted: [...missing].filter((name) => !(name in listed)).sort(),
    stale: Object.keys(listed).filter((name) => !missing.has(name)).sort(),
  };
}

const pinned = applyPinned([...rust, ...npm, ...renderer, ...host], { pins: PINS, dir: PINNED_TEXTS });
const all = pinned.packages;
const licenses = noticesText(all, { title: "Third-party licenses" });
const gaps = missingTexts(all);

// --check makes this usable as a gate: a generated file only stays accurate if something notices
// when it stops matching, and dependencies change far more often than anyone thinks to rerun a
// script. Exits non-zero when a committed file is out of date.
const check = process.argv.includes("--check");
for (const [file, text] of [[OUTPUT, body], [LICENSES_OUTPUT, licenses]]) {
  const name = basename(file);
  if (writeOrCheck(file, text, { check })) {
    console.log(check ? `${name} is up to date: ${summary()}.` : `Wrote ${file}: ${summary()}.`);
  } else {
    console.error(
      `${name} no longer matches the dependency graph.\n` +
        "Run `node scripts/generate-third-party-notices.mjs` and commit the result.",
    );
    process.exitCode = 1;
  }
}
if (gaps.unlisted.length) {
  console.error(
    "These packages publish without their license text and are not in scripts/notices-missing-texts.json:\n" +
      gaps.unlisted.map((n) => `  ${n}`).join("\n") +
      "\nAsk the publisher to ship the text, or list the package there with why.",
  );
  process.exitCode = 1;
}
if (gaps.stale.length) {
  console.error(
    "scripts/notices-missing-texts.json lists packages that now carry their text, or are gone — remove them:\n" +
      gaps.stale.map((n) => `  ${n}`).join("\n"),
  );
  process.exitCode = 1;
}
if (pinned.problems.length) {
  console.error(
    "Pinned license texts that could not be used:\n" +
      pinned.problems.map((x) => `  ${x.key}: ${x.problem}`).join("\n"),
  );
  process.exitCode = 1;
}
if (pinned.unused.length) {
  console.error(
    "scripts/notice-pins.json pins packages that are gone, moved to another version, or now carry their text — update or remove them:\n" +
      pinned.unused.map((k) => `  ${k}`).join("\n"),
  );
  process.exitCode = 1;
}
if (needsReview.length) process.exitCode = 1;
