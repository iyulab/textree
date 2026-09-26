#!/usr/bin/env node
/**
 * Which source an assembled sidecar was built from.
 *
 * Assembled sidecars live in gitignored folders and are not rebuilt when their source changes, so
 * after a branch switch they can be builds of other code entirely — and the checks that use them
 * (`host:smoke --exe`, the E2E suite) would then verify that other build and pass. Assembling
 * records a stamp; the checks compare it with what they mean to test:
 *
 *   host    against the host source as it stands now (src-host/)
 *   canopy  against the renderer commit a release ships (.github/canopy-ref) — the renderer is
 *           built from a separate checkout, which is often ahead of the pin on purpose
 *
 * Stamps live under .cache/, not beside the payloads: everything under src-tauri/resources/ is
 * bundled into the installer. Each is tied to its payload by hash, so a payload replaced by hand
 * reads as unknown rather than inheriting a stamp that is not its own.
 *
 *   node scripts/sidecar-provenance.mjs stamp host               # assemble-host-sidecar.ps1 calls it
 *   node scripts/sidecar-provenance.mjs stamp canopy <checkout>  # assemble-canopy-sidecar.ps1 calls it
 *   node scripts/sidecar-provenance.mjs check <host|canopy>      # prints the verdict; exit 1 unless current/pinned
 */
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export const SIDECARS = {
  host: { exe: "src-tauri/resources/host/textree-host.exe", source: "src-host" },
};
const CANOPY_STAGE = join(REPO, "src-tauri", "resources", "canopy");
const CANOPY_REF = join(REPO, ".github", "canopy-ref");

const stampPath = (kind) => join(REPO, ".cache", "sidecar-provenance", `${kind}.json`);
const readStamp = (kind) => (existsSync(stampPath(kind)) ? JSON.parse(readFileSync(stampPath(kind), "utf8")) : null);
function writeStamp(kind, record) {
  mkdirSync(dirname(stampPath(kind)), { recursive: true });
  writeFileSync(stampPath(kind), JSON.stringify(record, null, 2) + "\n");
}

function gitIn(cwd, ...args) {
  const r = spawnSync("git", args, { cwd, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`git ${args.join(" ")} failed: ${r.stderr.trim()}`);
  return r.stdout;
}

const sha256 = (data) => createHash("sha256").update(data).digest("hex");

/** Digest of uncommitted changes and untracked files under `pathspec`, or null when there are none. */
function pendingDigest(cwd, pathspec) {
  const diff = gitIn(cwd, "diff", "HEAD", "--binary", "--", pathspec);
  const untracked = gitIn(cwd, "ls-files", "--others", "--exclude-standard", "--", pathspec)
    .split("\n")
    .filter(Boolean)
    .sort();
  if (diff === "" && untracked.length === 0) return null;
  return sha256(diff + "\0" + untracked.map((f) => `${f}\0${sha256(readFileSync(join(cwd, f)))}`).join("\n"));
}

// ── host ────────────────────────────────────────────────────────────────────────────────────

/**
 * The source as it stands: the committed tree of the source folder plus a digest of anything
 * uncommitted in it (changes and untracked files), so a build of uncommitted work still matches
 * that same uncommitted work — and nothing else.
 */
export function sourceState(source) {
  const tree = gitIn(REPO, "rev-parse", `HEAD:${source}`).trim();
  const commit = gitIn(REPO, "rev-parse", "--short", "HEAD").trim();
  return { tree, commit, pending: pendingDigest(REPO, source) };
}

export function stamp(kind) {
  const { exe, source } = SIDECARS[kind];
  const record = { ...sourceState(source), exeSha256: sha256(readFileSync(join(REPO, exe))), assembledAt: new Date().toISOString() };
  writeStamp(kind, record);
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
  const now = sourceState(source);
  return { ...judge(readStamp(kind), sha256(readFileSync(staged)), now), now };
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

// ── canopy ──────────────────────────────────────────────────────────────────────────────────

/**
 * Digest of the assembled renderer's own files — its built code and package manifests. The
 * installed node_modules follow from package-lock.json and the Node runtime from the assembly
 * script's pinned version, so both are left out; hashing them would only make stamping slow.
 */
function canopyStageDigest() {
  const files = [];
  const walk = (dir) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, e.name);
      if (e.isDirectory()) {
        if (e.name !== "node_modules") walk(full);
      } else if (e.name !== "node.exe") {
        files.push(relative(CANOPY_STAGE, full).split("\\").join("/"));
      }
    }
  };
  walk(CANOPY_STAGE);
  return sha256(files.sort().map((f) => `${f}\0${sha256(readFileSync(join(CANOPY_STAGE, f)))}`).join("\n"));
}

export function stampCanopy(checkout) {
  const record = {
    commit: gitIn(checkout, "rev-parse", "HEAD").trim(),
    pending: pendingDigest(checkout, "."),
    stageSha256: canopyStageDigest(),
    assembledAt: new Date().toISOString(),
  };
  writeStamp("canopy", record);
  return record;
}

/**
 * Pure: stamp + stage digest + pinned commit → verdict.
 * pinned    — built from exactly the commit a release ships, with nothing uncommitted
 * unpinned  — built from another commit, or with uncommitted changes
 * unknown   — no stamp, or the stage is not the one the stamp describes
 */
export function judgeCanopy(record, stageSha256, pin) {
  if (!record || record.stageSha256 !== stageSha256) return { verdict: "unknown", pin };
  if (record.commit === pin && record.pending === null) return { verdict: "pinned", built: record, pin };
  return { verdict: "unpinned", built: record, pin };
}

export function checkCanopy() {
  const pin = readFileSync(CANOPY_REF, "utf8").trim();
  if (!existsSync(join(CANOPY_STAGE, "cli.js"))) return { verdict: "unknown", pin };
  return judgeCanopy(readStamp("canopy"), canopyStageDigest(), pin);
}

export function describeCanopy(result) {
  const short = (sha) => sha.slice(0, 7);
  switch (result.verdict) {
    case "pinned":
      return `canopy ${short(result.pin)}, the commit a release ships`;
    case "unpinned":
      return (
        `canopy ${short(result.built.commit)}${result.built.pending ? " + uncommitted changes" : ""}, ` +
        `not the commit a release ships (${short(result.pin)}) — for the shipped renderer, ` +
        `reassemble with scripts/assemble-canopy-sidecar.ps1 -Pinned`
      );
    default:
      return (
        `a renderer of unknown origin (no assembly record, or replaced since); a release ships ` +
        `canopy ${short(result.pin)} — reassemble with scripts/assemble-canopy-sidecar.ps1 -Pinned`
      );
  }
}

// ── command line ────────────────────────────────────────────────────────────────────────────

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [action, kind, checkout] = process.argv.slice(2);
  const valid =
    (kind === "host" && ["stamp", "check"].includes(action)) ||
    (kind === "canopy" && ((action === "stamp" && checkout) || action === "check"));
  if (!valid) {
    console.error(
      "usage: node scripts/sidecar-provenance.mjs <stamp|check> host\n" +
        "       node scripts/sidecar-provenance.mjs stamp canopy <checkout>\n" +
        "       node scripts/sidecar-provenance.mjs check canopy",
    );
    process.exit(2);
  }
  if (kind === "canopy" && action === "stamp") {
    const r = stampCanopy(resolve(checkout));
    console.log(`[provenance] canopy sidecar stamped: ${r.commit.slice(0, 7)}${r.pending ? " + uncommitted changes" : ""}`);
  } else if (kind === "canopy") {
    const r = checkCanopy();
    console.log(`[provenance] canopy sidecar: ${describeCanopy(r)}`);
    process.exit(r.verdict === "pinned" ? 0 : 1);
  } else if (action === "stamp") {
    const r = stamp(kind);
    console.log(`[provenance] ${kind} sidecar stamped: ${r.commit}${r.pending ? " + uncommitted changes" : ""}`);
  } else {
    const r = check(kind, join(REPO, SIDECARS[kind].exe));
    console.log(`[provenance] the ${kind} sidecar is ${describe(r)}`);
    process.exit(r.verdict === "current" ? 0 : 1);
  }
}
