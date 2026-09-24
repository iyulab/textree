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
 * Two more things are read from the restored dependency graph (src-host's project.assets.json,
 * so `dotnet restore` has to have run):
 *
 *   - Packages from these families that arrive only transitively — through a pinned package —
 *     are held to the same threshold. Nothing pins them, so nothing else would notice them
 *     falling behind; the fix is to move the package that brings them in.
 *   - A pinned package whose own declared floor on a sibling lags far behind that sibling's
 *     current line is reported (not failed): it is a sign the package itself is no longer kept
 *     up with, which only its maintainers can change.
 *
 * Usage: node scripts/check-pin-drift.mjs
 */

import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const PROPS = join(REPO, "src-host", "Directory.Packages.props");
const WAIVERS = join(REPO, "scripts", "pin-drift-waivers.json");
const ASSETS = join(REPO, "src-host", "src", "Textree.Host", "obj", "project.assets.json");

/** Package id prefixes whose pins are held to the current line. */
const FAMILIES = ["FluxIndex", "FileFlux", "FluxCurator", "LMSupply", "IronProw"];
const MAX_MINOR_GAP = 5;

/** Families that may also arrive only transitively, through a pinned package. */
const TRANSITIVE_FAMILIES = [...FAMILIES, "IronHive", "Flux", "FluxImprover", "FluxGuard"];

function parseVersion(v) {
  const [core, pre] = v.split("-", 2);
  const [major, minor, patch] = core.split(".").map((n) => Number.parseInt(n, 10));
  return { major, minor: minor ?? 0, patch: patch ?? 0, pre: pre ?? null, raw: v };
}

function inFamily(id, families = FAMILIES) {
  return families.some((f) => id === f || id.startsWith(`${f}.`));
}

/** Every package the restore resolved, as id → version. */
function readResolved() {
  if (!existsSync(ASSETS)) {
    throw new Error(`${ASSETS} not found — run \`dotnet restore src-host/TextreeHost.slnx\` first`);
  }
  const assets = JSON.parse(readFileSync(ASSETS, "utf8"));
  const resolved = new Map();
  for (const target of Object.values(assets.targets ?? {})) {
    for (const [key, entry] of Object.entries(target)) {
      if (entry.type !== "package") continue;
      const [id, version] = key.split("/");
      resolved.set(id, version);
    }
  }
  return resolved;
}

/** The lower bound a package's nuspec declares on each dependency, as id → version. */
async function declaredFloors(id, version) {
  const lower = id.toLowerCase();
  const url = `https://api.nuget.org/v3-flatcontainer/${lower}/${version.toLowerCase()}/${lower}.nuspec`;
  const res = await fetch(url, { signal: AbortSignal.timeout(15000) });
  if (!res.ok) throw new Error(`nuget.org answered ${res.status} for the ${id} ${version} nuspec`);
  const floors = new Map();
  for (const m of (await res.text()).matchAll(/<dependency\s+id="([^"]+)"\s+version="([^"]+)"/g)) {
    const bound = m[2].replace(/^[\[(]/, "").split(",")[0].trim();
    if (bound) floors.set(m[1], bound);
  }
  return floors;
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

const latestCache = new Map();
async function latestOf(id, allowPrerelease) {
  const key = `${id}|${allowPrerelease}`;
  if (!latestCache.has(key)) latestCache.set(key, await latestVersion(id, allowPrerelease));
  return latestCache.get(key);
}

/** Measures one package against its newest version and prints the verdict. Returns true on failure. */
async function judge(id, version, kind) {
  const have = parseVersion(version);
  const latest = parseVersion(await latestOf(id, have.pre !== null));
  const majorDiff = latest.major !== have.major;
  const minorGap = majorDiff ? Infinity : latest.minor - have.minor;
  const over = majorDiff || minorGap > MAX_MINOR_GAP;
  const waiver = waivers.get(id);
  waivers.delete(id);

  const gap = majorDiff ? "major version differs" : `${minorGap} minor behind`;
  const label = kind === "transitive" ? " (transitive)" : "";
  if (!over) {
    const note = latest.raw === have.raw ? "current" : gap;
    console.log(`  ok      ${id} ${have.raw}${label} (latest ${latest.raw}, ${note})`);
    if (waiver) console.log(`  stale   waiver for ${id} is no longer needed — remove it`);
    return false;
  }
  if (waiver && !waiver.expired) {
    console.log(`  waived  ${id} ${have.raw}${label} (latest ${latest.raw}, ${gap}) until ${waiver.until}: ${waiver.reason}`);
    return false;
  }
  const why = waiver ? `waiver expired on ${waiver.until}` : "no waiver";
  const fix = kind === "transitive" ? " — move the pinned package that brings it in" : "";
  console.log(`  FAIL    ${id} ${have.raw}${label} (latest ${latest.raw}, ${gap}; ${why})${fix}`);
  return true;
}

let failures = 0;
for (const pin of pins) {
  if (await judge(pin.id, pin.version, "pinned")) failures++;
}

const resolved = readResolved();
const pinnedIds = new Set(pins.map((p) => p.id));
const transitive = [...resolved].filter(([id]) => !pinnedIds.has(id) && inFamily(id, TRANSITIVE_FAMILIES));
for (const [id, version] of transitive) {
  if (await judge(id, version, "transitive")) failures++;
}

// Reported, never failed: a stale floor is the package's own maintainers' to move.
for (const pin of pins) {
  const floors = await declaredFloors(pin.id, pin.version);
  for (const [dep, floor] of floors) {
    if (!inFamily(dep, TRANSITIVE_FAMILIES)) continue;
    const have = parseVersion(floor);
    const latest = parseVersion(await latestOf(dep, have.pre !== null));
    const gap = latest.major !== have.major ? Infinity : latest.minor - have.minor;
    if (gap > MAX_MINOR_GAP) {
      const said = gap === Infinity ? "a major version" : `${gap} minor versions`;
      console.log(`  note    ${pin.id} ${pin.version} still declares ${dep} >= ${floor}, ${said} behind ${latest.raw} — is it still kept up with?`);
    }
  }
}

for (const id of waivers.keys()) {
  console.log(`  stale   waiver for ${id} matches no tracked package — remove it`);
}

if (failures > 0) {
  console.error(`
${failures} package(s) drifted past the threshold (major difference or more than ${MAX_MINOR_GAP} minor versions).`);
  console.error("Upgrade them, or add a waiver with an expiry and a reason to scripts/pin-drift-waivers.json.");
  process.exit(1);
}
console.log(`
Pin drift within threshold: ${pins.length} pinned, ${transitive.length} transitive.`);
