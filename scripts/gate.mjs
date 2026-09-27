/*
 * Runs the merge gate locally, in the order CI runs it, plus the checks CI does not run
 * (clippy, the host test suite). Every step runs even after one fails, so a single pass reports
 * everything that is red instead of stopping at the first failure.
 *
 * Usage:
 *   npm run gate                       all merge steps
 *   npm run gate -- --ship             the pre-release gate: assemble both sidecars first (the
 *                                      renderer at the commit a release ships), then every merge
 *                                      step, then the E2E suite and host:smoke on the assembled host
 *   npm run gate -- --list             print the step names
 *   npm run gate -- --only check,unit  just these steps (release steps may be named here too)
 *   npm run gate -- --skip audit       all but these steps
 *
 * Without --ship it expects the sidecars to be assembled already (scripts/assemble-*-sidecar.ps1) —
 * the Rust build bundles them. The release steps stay out of the plain gate: host:smoke needs a
 * model cache and E2E needs a desktop session, which a merge gate must not depend on. With --ship
 * they run in the order that keeps each honest: the assemblies come first, so the E2E suite renders
 * with the shipped canopy and host:smoke checks a build of the current source.
 */

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { runningDevApps } from "./dev-processes.mjs";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const HOST_SOLUTION = "src-host/TextreeHost.slnx";
const CARGO_MANIFEST = "src-tauri/Cargo.toml";

/** Same order as .github/workflows/ci.yml; `local` marks steps CI does not run. */
const STEPS = [
  { name: "audit", cmd: "npm audit --omit=dev --audit-level=high" },
  { name: "tokens", cmd: "npm run tokens:check" },
  { name: "check", cmd: "npm run check" },
  { name: "unit", cmd: "npm run test:unit" },
  { name: "build", cmd: "npm run build" },
  { name: "restore", cmd: `dotnet restore ${HOST_SOLUTION}` },
  { name: "pins", cmd: "npm run pins:check" },
  { name: "notices", cmd: "npm run notices:check" }, // reads the host's restored graph
  { name: "host-test", cmd: `dotnet test ${HOST_SOLUTION} --no-restore`, local: true },
  { name: "cargo-test", cmd: `cargo test --manifest-path ${CARGO_MANIFEST}` },
  { name: "clippy", cmd: `cargo clippy --manifest-path ${CARGO_MANIFEST} --all-targets -- -D warnings`, local: true },
];

/** Added by --ship around the merge steps (`at`: before or after them). All local. */
const RELEASE_STEPS = [
  { name: "assemble-canopy", cmd: "pwsh -NoProfile -File scripts/assemble-canopy-sidecar.ps1", at: "before" },
  { name: "assemble-host", cmd: "pwsh -NoProfile -File scripts/assemble-host-sidecar.ps1", at: "before" },
  { name: "e2e", cmd: "npm run e2e:run", at: "after" },
  { name: "smoke", cmd: "npm run host:smoke -- --exe src-tauri/resources/host/textree-host.exe", at: "after" },
].map((s) => ({ ...s, local: true }));

const SHIP = process.argv.includes("--ship");
const ALL_STEPS = [
  ...RELEASE_STEPS.filter((s) => s.at === "before"),
  ...STEPS,
  ...RELEASE_STEPS.filter((s) => s.at === "after"),
];
const releaseStep = (name) => RELEASE_STEPS.some((s) => s.name === name);

function parseList(flag) {
  const i = process.argv.indexOf(flag);
  if (i === -1) return null;
  const names = (process.argv[i + 1] ?? "").split(",").filter(Boolean);
  const unknown = names.filter((n) => !ALL_STEPS.some((s) => s.name === n));
  if (unknown.length > 0) {
    console.error(`[gate] unknown step(s): ${unknown.join(", ")} — see --list`);
    process.exit(2);
  }
  return names;
}

if (process.argv.includes("--list")) {
  for (const s of ALL_STEPS) {
    const tag = releaseStep(s.name) ? "   (--ship)" : s.local ? "   (local only)" : "";
    console.log(`${s.name.padEnd(15)} ${s.cmd}${tag}`);
  }
  process.exit(0);
}

const only = parseList("--only");
const skip = parseList("--skip") ?? [];
// Release steps run with --ship, or when named in --only; merge steps as before.
const selected = ALL_STEPS.filter(
  (s) => (only ? only.includes(s.name) : SHIP || !releaseStep(s.name)) && !skip.includes(s.name),
);

// A running dev build holds its own executable open, and cargo then fails to replace it with
// "Access is denied (os error 5)" — a failure that names neither the cause nor the fix.
if (selected.some((s) => s.name.startsWith("cargo") || s.name === "clippy")) {
  // Only the app's own executable is what cargo rebuilds; a host sidecar left running does not block it.
  const apps = runningDevApps(REPO).filter((a) => a.name === "textree.exe");
  if (apps.length > 0) {
    console.error(
      `[gate] the dev build is running (pid ${apps.map((a) => a.pid).join(", ")}); cargo cannot replace ` +
        `its executable. Close it (or run \`npm run e2e:run\`, which stops what it starts) and retry.`,
    );
    process.exit(2);
  }
}

for (const [sidecar, assembledBy] of [
  ["src-tauri/resources/canopy/node_modules/@iyulab/canopy/dist/cli.js", "assemble-canopy"],
  ["src-tauri/resources/host/textree-host.exe", "assemble-host"],
]) {
  if (!existsSync(join(REPO, sidecar)) && !selected.some((s) => s.name === assembledBy)) {
    console.warn(`[gate] warning: ${sidecar} is missing — assemble the sidecars first or the Rust steps may fail`);
  }
}

const results = [];
for (const step of selected) {
  console.log(`\n[gate] ── ${step.name}: ${step.cmd}`);
  const started = Date.now();
  const run = spawnSync(step.cmd, { cwd: REPO, stdio: "inherit", shell: true });
  const seconds = (Date.now() - started) / 1000;
  results.push({ ...step, ok: run.status === 0, seconds });
}

console.log("\n[gate] summary");
for (const r of results) {
  console.log(`  ${r.ok ? "PASS" : "FAIL"}  ${r.name.padEnd(15)} ${r.seconds.toFixed(1).padStart(7)}s${r.local ? "  (local only)" : ""}`);
}
const failed = results.filter((r) => !r.ok);
console.log(failed.length === 0 ? "[gate] all green" : `[gate] ${failed.length} failed: ${failed.map((r) => r.name).join(", ")}`);
process.exit(failed.length === 0 ? 0 : 1);
