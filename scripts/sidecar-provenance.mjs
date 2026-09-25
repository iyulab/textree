#!/usr/bin/env node
/**
 * Which source an assembled sidecar was built from.
 *
 * The assembled host lives in a gitignored folder and is not rebuilt when the source changes, so
 * after a branch switch it can be a build of other code entirely — and `host:smoke --exe` would
 * then verify that other build and pass. Assembling records a stamp here; the smoke compares it
 * with the source as it stands now.
 *
 * The stamp lives under .cache/, not beside the exe: everything under src-tauri/resources/host/
 * is bundled into the installer. It is tied to the exe by hash, so an exe copied in by hand reads
 * as unknown rather than inheriting a stamp that is not its own.
 *
 *   node scripts/sidecar-provenance.mjs stamp host    # after assembling (assemble-host-sidecar.ps1 calls it)
 *   node scripts/sidecar-provenance.mjs check host    # prints the verdict; exit 1 unless current
 */
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export const SIDECARS = {
  host: { exe: "src-tauri/resources/host/textree-host.exe", source: "src-host" },
};

const stampPath = (kind) => join(REPO, ".cache", "sidecar-provenance", `${kind}.json`);

function git(...args) {
  const r = spawnSync("git", args, { cwd: REPO, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`git ${args.join(" ")} failed: ${r.stderr.trim()}`);
  return r.stdout;
}

const sha256 = (data) => createHash("sha256").update(data).digest("hex");

/**
 * The source as it stands: the committed tree of the source folder plus a digest of anything
 * uncommitted in it (changes and untracked files), so a build of uncommitted work still matches
 * that same uncommitted work — and nothing else.
 */
export function sourceState(source) {
  const tree = git("rev-parse", `HEAD:${source}`).trim();
  const commit = git("rev-parse", "--short", "HEAD").trim();
  const diff = git("diff", "HEAD", "--binary", "--", source);
  const untracked = git("ls-files", "--others", "--exclude-standard", "--", source)
    .split("\n")
    .filter(Boolean)
    .sort();
  const pending = diff === "" && untracked.length === 0
    ? null
    : sha256(diff + "\0" + untracked.map((f) => `${f}\0${sha256(readFileSync(join(REPO, f)))}`).join("\n"));
  return { tree, commit, pending };
}

export function stamp(kind) {
  const { exe, source } = SIDECARS[kind];
  const state = sourceState(source);
  const record = { ...state, exeSha256: sha256(readFileSync(join(REPO, exe))), assembledAt: new Date().toISOString() };
  mkdirSync(dirname(stampPath(kind)), { recursive: true });
  writeFileSync(stampPath(kind), JSON.stringify(record, null, 2) + "\n");
  return record;
}

/**
 * Pure: stamp + exe hash + current source → verdict.
 * current  — built from exactly the source as it stands
 * stale    — built from other source (another commit, or other uncommitted changes)
 * unknown  — no stamp, or the exe is not the one the stamp describes
 */
export function judge(record, exeSha256, now) {
  if (!record || record.exeSha256 !== exeSha256) return { verdict: "unknown" };
  if (record.tree === now.tree && record.pending === now.pending) return { verdict: "current", built: record };
  return { verdict: "stale", built: record };
}

/** Verdict for `exePath`, or null when it is not the staged sidecar (nothing to compare against). */
export function check(kind, exePath) {
  const { exe, source } = SIDECARS[kind];
  const staged = join(REPO, exe);
  if (resolve(exePath).toLowerCase() !== staged.toLowerCase()) return null;
  if (!existsSync(staged)) return { verdict: "unknown" };
  const record = existsSync(stampPath(kind)) ? JSON.parse(readFileSync(stampPath(kind), "utf8")) : null;
  const now = sourceState(source);
  return { ...judge(record, sha256(readFileSync(staged)), now), now };
}

export function describe(result) {
  const at = (s) => `${s.commit}${s.pending ? " + uncommitted changes" : ""}`;
  switch (result.verdict) {
    case "current":
      return `built from the current source (${at(result.built)})`;
    case "stale":
      return `built from ${at(result.built)}, but the source is now ${at(result.now)} — reassemble with scripts/assemble-host-sidecar.ps1`;
    default:
      return "of unknown origin (no assembly record, or replaced since) — reassemble with scripts/assemble-host-sidecar.ps1";
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [action, kind] = process.argv.slice(2);
  if (!SIDECARS[kind] || !["stamp", "check"].includes(action)) {
    console.error(`usage: node scripts/sidecar-provenance.mjs <stamp|check> <${Object.keys(SIDECARS).join("|")}>`);
    process.exit(2);
  }
  if (action === "stamp") {
    const r = stamp(kind);
    console.log(`[provenance] ${kind} sidecar stamped: ${r.commit}${r.pending ? " + uncommitted changes" : ""}`);
  } else {
    const r = check(kind, join(REPO, SIDECARS[kind].exe));
    console.log(`[provenance] the ${kind} sidecar is ${describe(r)}`);
    process.exit(r.verdict === "current" ? 0 : 1);
  }
}
