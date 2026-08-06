/*
 * Regenerates THIRD-PARTY-NOTICES.md from the dependency graphs that actually ship.
 *
 * A hand-written inventory of several hundred dependencies is out of date the moment anything is
 * added, so the file is generated. Run this whenever dependencies change:
 *
 *   node scripts/generate-third-party-notices.mjs
 *
 * Two graphs feed it, because both end up inside the installed application:
 *   - Rust: the normal-dependency closure of the binary, resolved for the release target. Build-
 *     and dev-dependencies are excluded; they run during the build and are not distributed.
 *   - npm: the production-dependency closure, whose code is bundled into the web assets.
 *
 * The prelude is kept by hand: the native libraries compiled into the git engine are not visible
 * to either package manager, and their terms constrain how the whole distribution may be shipped.
 */

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const TARGET = "x86_64-pc-windows-msvc";
const OUTPUT = join(ROOT, "THIRD-PARTY-NOTICES.md");
const PRELUDE = join(ROOT, "scripts", "third-party-notices-prelude.md");

/**
 * License expressions that are compatible with this application's GPL-3.0-only license, either
 * because they are permissive or because they carry an explicit exception. Anything outside this
 * set is reported rather than silently listed: a copyleft term we cannot satisfy is a licensing
 * defect, not a formatting one.
 */
const KNOWN_COMPATIBLE = new Set([
  "0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "BSL-1.0", "CC0-1.0", "ISC", "MIT",
  "MIT-0", "MPL-2.0", "Python-2.0", "Unicode-3.0", "Unicode-DFS-2016", "Zlib", "zlib-acknowledgement",
  "GPL-3.0-only", "GPL-3.0-or-later", "LGPL-2.1-or-later", "LGPL-3.0-or-later",
  "Unlicense", "WTFPL", "CDLA-Permissive-2.0", "OpenSSL",
]);

/**
 * Splits an SPDX expression into the individual identifiers it mentions. A slash is treated as a
 * separator too: crates predating SPDX expressions still declare `MIT/Apache-2.0`, and reading
 * that as one unknown identifier would flag two dozen ordinary permissive crates for review.
 * A screen that reports things that are fine is a screen nobody reads.
 */
function identifiersIn(expression) {
  return expression
    .replace(/[()]/g, " ")
    .split(/\s*\/\s*|\s+(?:OR|AND|WITH)\s+/i)
    .map((s) => s.trim())
    .filter(Boolean);
}

/** True when at least one alternative of an SPDX expression is known to be compatible. */
function isSatisfiable(expression) {
  // An OR gives a choice, so one compatible alternative is enough. AND/WITH are approximated the
  // same way on purpose: this is a screen that surfaces candidates for review, not a legal check.
  return identifiersIn(expression).some((id) => KNOWN_COMPATIBLE.has(id));
}

function rustDependencies() {
  const raw = execFileSync(
    "cargo",
    ["metadata", "--format-version", "1", "--filter-platform", TARGET],
    { cwd: join(ROOT, "src-tauri"), encoding: "utf8", maxBuffer: 128 * 1024 * 1024 },
  );
  const meta = JSON.parse(raw);
  const byId = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));

  // Walk only normal dependencies: a build-dependency's code runs during compilation and is not
  // part of what is installed.
  const reached = new Set();
  const queue = [meta.resolve.root];
  while (queue.length) {
    const id = queue.shift();
    if (reached.has(id)) continue;
    reached.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      if (dep.dep_kinds.some((k) => k.kind === null)) queue.push(dep.pkg);
    }
  }
  reached.delete(meta.resolve.root);

  return [...reached]
    .map((id) => byId.get(id))
    .filter(Boolean)
    .map((p) => ({
      name: p.name,
      version: p.version,
      license: p.license ?? "(not declared)",
      url: p.repository ?? `https://crates.io/crates/${p.name}`,
    }))
    .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
}

/**
 * Reads the production closure straight out of the lockfile rather than shelling out to npm: the
 * lockfile already records exactly what an install resolves to, and it makes this script runnable
 * without a package manager on the path.
 */
function npmDependencies() {
  const lock = JSON.parse(readFileSync(join(ROOT, "package-lock.json"), "utf8"));
  const entries = [];
  for (const [path, entry] of Object.entries(lock.packages)) {
    if (!path || entry.dev || entry.link) continue; // "" is the application itself
    const name = path.slice(path.lastIndexOf("node_modules/") + "node_modules/".length);

    // The lockfile carries a license for most packages; fall back to the installed manifest, which
    // also supplies the upstream URL the lockfile does not record.
    let manifest = {};
    try {
      manifest = JSON.parse(readFileSync(join(ROOT, path, "package.json"), "utf8"));
    } catch {
      // Not installed right now: the lockfile entry still describes what ships.
    }
    const declared =
      entry.license ??
      manifest.license ??
      (Array.isArray(manifest.licenses) ? manifest.licenses.map((l) => l.type).join(" OR ") : null);
    const repo =
      typeof manifest.repository === "string" ? manifest.repository : manifest.repository?.url;

    entries.push({
      name,
      version: entry.version ?? manifest.version ?? "(unknown)",
      license: typeof declared === "string" ? declared : "(not declared)",
      url: (repo ?? `https://www.npmjs.com/package/${name}`)
        .replace(/^git\+/, "")
        .replace(/\.git$/, ""),
    });
  }
  return entries.sort(
    (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version),
  );
}

function table(entries) {
  const rows = entries.map(
    (e) => `| \`${e.name}\` | ${e.version} | ${e.license} | ${e.url} |`,
  );
  return ["| Component | Version | License | Upstream |", "| --- | --- | --- | --- |", ...rows].join("\n");
}

const rust = rustDependencies();
const npm = npmDependencies();

const needsReview = [...rust, ...npm].filter((e) => !isSatisfiable(e.license));
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
  "The full license text for each component is available at the upstream location listed above.",
  "",
].join("\n");

// --check makes this usable as a gate: a generated file only stays accurate if something notices
// when it stops matching, and dependencies change far more often than anyone thinks to rerun a
// script. Exits non-zero when the committed file is out of date.
if (process.argv.includes("--check")) {
  const committed = readFileSync(OUTPUT, "utf8");
  if (committed === body) {
    console.log(`THIRD-PARTY-NOTICES.md is up to date: ${rust.length} crates, ${npm.length} web packages.`);
  } else {
    console.error(
      "THIRD-PARTY-NOTICES.md no longer matches the dependency graph.\n" +
        "Run `node scripts/generate-third-party-notices.mjs` and commit the result.",
    );
    process.exitCode = 1;
  }
} else {
  writeFileSync(OUTPUT, body, "utf8");
  console.log(`Wrote ${OUTPUT}: ${rust.length} crates, ${npm.length} web packages.`);
}
if (needsReview.length) process.exitCode = 1;
