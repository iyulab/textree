/*
 * Runs the merge gate locally, in the order CI runs it, plus the checks CI does not run
 * (clippy, the host test suite). Every step runs even after one fails, so a single pass reports
 * everything that is red instead of stopping at the first failure.
 *
 * Usage:
 *   npm run gate                       all steps
 *   npm run gate -- --list             print the step names
 *   npm run gate -- --only check,unit  just these steps
 *   npm run gate -- --skip audit       all but these steps
 *
 * Expects the sidecars to be assembled already (scripts/assemble-*-sidecar.ps1) — the Rust build
 * bundles them. Release-only checks (host:smoke against the assembled executable) stay separate:
 * they need a model cache, which a gate must not depend on.
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
  { name: "notices", cmd: "npm run notices:check" },
  { name: "tokens", cmd: "npm run tokens:check" },
  { name: "check", cmd: "npm run check" },
  { name: "unit", cmd: "npm run test:unit" },
  { name: "build", cmd: "npm run build" },
  { name: "restore", cmd: `dotnet restore ${HOST_SOLUTION}` },
  { name: "pins", cmd: "npm run pins:check" },
  { name: "host-test", cmd: `dotnet test ${HOST_SOLUTION} --no-restore`, local: true },
  { name: "cargo-test", cmd: `cargo test --manifest-path ${CARGO_MANIFEST}` },
  { name: "clippy", cmd: `cargo clippy --manifest-path ${CARGO_MANIFEST} --all-targets -- -D warnings`, local: true },
];

function parseList(flag) {
  const i = process.argv.indexOf(flag);
  if (i === -1) return null;
  const names = (process.argv[i + 1] ?? "").split(",").filter(Boolean);
  const unknown = names.filter((n) => !STEPS.some((s) => s.name === n));
  if (unknown.length > 0) {
    console.error(`[gate] unknown step(s): ${unknown.join(", ")} — see --list`);
    process.exit(2);
  }
  return names;
}

if (process.argv.includes("--list")) {
  for (const s of STEPS) console.log(`${s.name.padEnd(11)} ${s.cmd}${s.local ? "   (local only)" : ""}`);
  process.exit(0);
}

const only = parseList("--only");
const skip = parseList("--skip") ?? [];
const selected = STEPS.filter((s) => (only ? only.includes(s.name) : true) && !skip.includes(s.name));

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

for (const sidecar of ["src-tauri/resources/canopy/cli.js", "src-tauri/resources/host/textree-host.exe"]) {
  if (!existsSync(join(REPO, sidecar))) {
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
  console.log(`  ${r.ok ? "PASS" : "FAIL"}  ${r.name.padEnd(11)} ${r.seconds.toFixed(1).padStart(7)}s${r.local ? "  (local only)" : ""}`);
}
const failed = results.filter((r) => !r.ok);
console.log(failed.length === 0 ? "[gate] all green" : `[gate] ${failed.length} failed: ${failed.map((r) => r.name).join(", ")}`);
process.exit(failed.length === 0 ? 0 : 1);
