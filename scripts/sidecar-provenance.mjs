#!/usr/bin/env node
/**
 * Which source an assembled sidecar was built from.
 *
 * Assembled sidecars live in gitignored folders and are not rebuilt when their source changes, so
 * after a branch switch they can be builds of other code entirely — and the checks that use them
 * (`host:smoke --exe`, the E2E suite) would then verify that other build and pass. Assembling
 * records a stamp; the checks compare it with what they mean to test:
 *
 *   host    against the host source as it stands now (src-host/) and the app version it is
 *           stamped with (src-tauri/tauri.conf.json — assembling passes it to the build)
 *   canopy  against the renderer release the app ships (canopy-sidecar/) — it can also be built
 *           from a source checkout, to try a renderer change before it is released
 *
 * Stamps live under .cache/, not beside the payloads: everything under src-tauri/resources/ is
 * bundled into the installer. Each is tied to its payload by hash, so a payload replaced by hand
 * reads as unknown rather than inheriting a stamp that is not its own.
 *
 *   node scripts/sidecar-provenance.mjs stamp host               # assemble-host-sidecar.ps1 calls it
 *   node scripts/sidecar-provenance.mjs stamp canopy [checkout]  # assemble-canopy-sidecar.ps1 calls it
 *   node scripts/sidecar-provenance.mjs check <host|canopy>      # prints the verdict; exit 1 unless current/pinned
 */
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { CANOPY_CLI, CANOPY_MANIFEST_DIR, CANOPY_STAGE, canopyPin } from "./canopy-stage.mjs";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export const SIDECARS = {
  host: { exe: "src-tauri/resources/host/textree-host.exe", source: "src-host" },
};
const APP_CONFIG = join(REPO, "src-tauri", "tauri.conf.json");

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
 * that same uncommitted work — and nothing else. The app version is part of it: the build is
 * stamped with it, so a version bump outside the source folder still makes an older build stale.
 */
export function sourceState(source) {
  const tree = gitIn(REPO, "rev-parse", `HEAD:${source}`).trim();
  const commit = gitIn(REPO, "rev-parse", "--short", "HEAD").trim();
  const version = JSON.parse(readFileSync(APP_CONFIG, "utf8")).version ?? null;
  return { tree, commit, pending: pendingDigest(REPO, source), version };
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
 * stale    — built from other source (another commit, or other uncommitted changes) or for
 *            another app version
 * unknown  — no stamp, or the exe is not the one the stamp describes
 */
export function judge(record, exeSha256, now) {
  if (!record || record.exeSha256 !== exeSha256) return { verdict: "unknown" };
  // A stamp from before versions were recorded has no `version` — it cannot vouch for one.
  if (record.tree === now.tree && record.pending === now.pending && record.version === now.version) {
    return { verdict: "current", built: record };
  }
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
  const at = (s) => `${s.commit}${s.pending ? " + uncommitted changes" : ""}, version ${s.version ?? "unrecorded"}`;
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

const CANOPY_IN_STAGE = join(CANOPY_STAGE, "node_modules", "@iyulab", "canopy");

/**
 * Digest of what decides the assembled renderer: the lock file it was installed from and the
 * renderer package's own files. The rest of node_modules follows from the lock file and the Node
 * runtime from the assembly script's pinned version; hashing them would only make stamping slow.
 */
function canopyStageDigest() {
  const files = [join(CANOPY_STAGE, "package-lock.json")];
  const walk = (dir) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, e.name);
      if (e.isDirectory()) {
        if (e.name !== "node_modules") walk(full);
      } else {
        files.push(full);
      }
    }
  };
  walk(CANOPY_IN_STAGE);
  const named = files.map((f) => [relative(CANOPY_STAGE, f).split("\\").join("/"), f]);
  named.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
  return sha256(named.map(([n, f]) => `${n}\0${sha256(readFileSync(f))}`).join("\n"));
}

/** The installed renderer's version, or null when the stage holds none. */
function installedCanopyVersion() {
  const manifest = join(CANOPY_IN_STAGE, "package.json");
  return existsSync(manifest) ? JSON.parse(readFileSync(manifest, "utf8")).version : null;
}

/**
 * `checkout`: the source checkout it was built from, or undefined for the pinned release. A stage
 * installed from the registry counts as the release only when its lock file is the committed one.
 */
export function stampCanopy(checkout) {
  const fromCheckout = checkout !== undefined;
  const committedLock = readFileSync(join(CANOPY_MANIFEST_DIR, "package-lock.json"));
  const record = {
    source: fromCheckout ? "checkout" : "registry",
    version: installedCanopyVersion(),
    commit: fromCheckout ? gitIn(checkout, "rev-parse", "HEAD").trim() : null,
    pending: fromCheckout ? pendingDigest(checkout, ".") : null,
    lockSha256: fromCheckout ? null : sha256(committedLock),
    stageSha256: canopyStageDigest(),
    assembledAt: new Date().toISOString(),
  };
  writeStamp("canopy", record);
  return record;
}

/**
 * Pure: stamp + stage digest + pin + committed lock digest → verdict.
 * pinned    — the pinned release, installed from the committed lock file
 * unpinned  — built from a source checkout, or installed from another lock file
 * unknown   — no stamp, or the stage is not the one the stamp describes
 */
export function judgeCanopy(record, stageSha256, pin, lockSha256) {
  if (!record || record.stageSha256 !== stageSha256) return { verdict: "unknown", pin };
  if (record.source === "registry" && record.version === pin && record.lockSha256 === lockSha256) {
    return { verdict: "pinned", built: record, pin };
  }
  return { verdict: "unpinned", built: record, pin };
}

export function checkCanopy() {
  const pin = canopyPin();
  if (!existsSync(CANOPY_CLI)) return { verdict: "unknown", pin };
  const lock = sha256(readFileSync(join(CANOPY_MANIFEST_DIR, "package-lock.json")));
  return judgeCanopy(readStamp("canopy"), canopyStageDigest(), pin, lock);
}

export function describeCanopy(result) {
  const reassemble = "reassemble with scripts/assemble-canopy-sidecar.ps1";
  switch (result.verdict) {
    case "pinned":
      return `canopy ${result.pin}, the release the app ships`;
    case "unpinned": {
      const b = result.built;
      const what =
        b.source === "checkout"
          ? `canopy ${b.version ?? "?"} built from ${b.commit?.slice(0, 7) ?? "a checkout"}${b.pending ? " + uncommitted changes" : ""}`
          : `canopy ${b.version ?? "?"} from another lock file`;
      return `${what}, not the release the app ships (${result.pin}) — for the shipped renderer, ${reassemble}`;
    }
    default:
      return `a renderer of unknown origin (no assembly record, or replaced since); the app ships canopy ${result.pin} — ${reassemble}`;
  }
}

// ── command line ────────────────────────────────────────────────────────────────────────────

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [action, kind, checkout] = process.argv.slice(2);
  const valid =
    (kind === "host" && ["stamp", "check"].includes(action)) ||
    (kind === "canopy" && ["stamp", "check"].includes(action));
  if (!valid) {
    console.error(
      "usage: node scripts/sidecar-provenance.mjs <stamp|check> host\n" +
        "       node scripts/sidecar-provenance.mjs stamp canopy [checkout]\n" +
        "       node scripts/sidecar-provenance.mjs check canopy",
    );
    process.exit(2);
  }
  if (kind === "canopy" && action === "stamp") {
    const r = stampCanopy(checkout ? resolve(checkout) : undefined);
    const from = r.source === "checkout" ? `built from ${r.commit.slice(0, 7)}${r.pending ? " + uncommitted changes" : ""}` : "from the registry";
    console.log(`[provenance] canopy sidecar stamped: ${r.version} ${from}`);
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
