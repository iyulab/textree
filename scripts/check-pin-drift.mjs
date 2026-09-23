/*
 * Pin drift check for the host's package families.
 *
 * The host consumes a few package families that release often and are meant to be consumed at
 * their current line. Falling behind them is silent: restore, build and tests all stay green on an
 * old pin, so the gap only grows until an upgrade becomes a migration. This compares each pin in
 * src-host/Directory.Packages.props that belongs to one of those families against the newest
 * version on nuget.org and fails when:
 *
 *   - the major version differs (always a human decision), or
 *   - the pin is more than MAX_MINOR_GAP minor versions behind,
 *
 * unless the pin is listed in scripts/pin-drift-waivers.json with an expiry that has not passed.
 * A waiver without an expiry is refused: it would be a permanent suppression under another name.
 * A waiver that is no longer needed is reported so it gets removed rather than left to mask the
 * next drift.
 *
 * Smaller gaps are reported and pass — drift alone is not a defect, drift nobody sees is.
 *
 * Usage: node scripts/check-pin-drift.mjs
 */

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const PROPS = join(REPO, "src-host", "Directory.Packages.props");
const WAIVERS = join(REPO, "scripts", "pin-drift-waivers.json");

/** Package id prefixes whose pins are held to the current line. */
const FAMILIES = ["FluxIndex", "FileFlux", "FluxCurator", "LMSupply", "IronProw"];
const MAX_MINOR_GAP = 5;

function parseVersion(v) {
  const [core, pre] = v.split("-", 2);
  const [major, minor, patch] = core.split(".").map((n) => Number.parseInt(n, 10));
  return { major, minor: minor ?? 0, patch: patch ?? 0, pre: pre ?? null, raw: v };
}

function inFamily(id) {
  return FAMILIES.some((f) => id === f || id.startsWith(`${f}.`));
}

function readPins() {
  const xml = readFileSync(PROPS, "utf8");
  const pins = [];
  for (const m of xml.matchAll(/<PackageVersion\s+Include="([^"]+)"\s+Version="([^"]+)"/g)) {
    if (inFamily(m[1])) pins.push({ id: m[1], version: m[2] });
  }
  return pins;
}

function readWaivers(today) {
  const list = JSON.parse(readFileSync(WAIVERS, "utf8"));
  const byId = new Map();
  for (const w of list) {
    if (!w.package || !w.until || !w.reason) {
      throw new Error(`pin-drift-waivers.json: every waiver needs package, until and reason — got ${JSON.stringify(w)}`);
    }
    if (!/^\d{4}-\d{2}-\d{2}$/.test(w.until)) {
      throw new Error(`pin-drift-waivers.json: "until" must be YYYY-MM-DD — got "${w.until}" for ${w.package}`);
    }
    byId.set(w.package, { ...w, expired: w.until < today });
  }
  return byId;
}

async function latestVersion(id, allowPrerelease) {
  const url = `https://api.nuget.org/v3-flatcontainer/${id.toLowerCase()}/index.json`;
  const res = await fetch(url, { signal: AbortSignal.timeout(15000) });
  if (!res.ok) throw new Error(`nuget.org answered ${res.status} for ${id}`);
  const { versions } = await res.json();
  const candidates = versions.filter((v) => allowPrerelease || !v.includes("-"));
  if (candidates.length === 0) throw new Error(`nuget.org lists no ${allowPrerelease ? "" : "stable "}version of ${id}`);
  return candidates[candidates.length - 1];
}

const today = new Date().toISOString().slice(0, 10);
const waivers = readWaivers(today);
const pins = readPins();
if (pins.length === 0) {
  throw new Error(`No pins from the tracked families found in ${PROPS} — the family list or the file moved`);
}

let failures = 0;
for (const pin of pins) {
  const have = parseVersion(pin.version);
  const latest = parseVersion(await latestVersion(pin.id, have.pre !== null));
  const majorDiff = latest.major !== have.major;
  const minorGap = majorDiff ? Infinity : latest.minor - have.minor;
  const over = majorDiff || minorGap > MAX_MINOR_GAP;
  const waiver = waivers.get(pin.id);
  waivers.delete(pin.id);

  const gap = majorDiff ? "major version differs" : `${minorGap} minor behind`;
  if (!over) {
    const note = latest.raw === have.raw ? "current" : gap;
    console.log(`  ok      ${pin.id} ${have.raw} (latest ${latest.raw}, ${note})`);
    if (waiver) console.log(`  stale   waiver for ${pin.id} is no longer needed — remove it`);
  } else if (waiver && !waiver.expired) {
    console.log(`  waived  ${pin.id} ${have.raw} (latest ${latest.raw}, ${gap}) until ${waiver.until}: ${waiver.reason}`);
  } else {
    failures++;
    const why = waiver ? `waiver expired on ${waiver.until}` : "no waiver";
    console.log(`  FAIL    ${pin.id} ${have.raw} (latest ${latest.raw}, ${gap}; ${why})`);
  }
}
for (const id of waivers.keys()) {
  console.log(`  stale   waiver for ${id} matches no tracked pin — remove it`);
}

if (failures > 0) {
  console.error(`\n${failures} pin(s) drifted past the threshold (major difference or more than ${MAX_MINOR_GAP} minor versions).`);
  console.error("Upgrade them, or add a waiver with an expiry and a reason to scripts/pin-drift-waivers.json.");
  process.exit(1);
}
console.log(`\nPin drift within threshold: ${pins.length} tracked pins.`);
