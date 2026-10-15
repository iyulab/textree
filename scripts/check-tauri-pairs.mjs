/**
 * The Tauri crates and their npm halves are released together, and `tauri build` refuses to build
 * when a pair is on different minor releases. Development builds do not check, so a lock-file
 * update on one side alone stays green until the release build fails. This compares the resolved
 * versions on both sides — `src-tauri/Cargo.lock` and `package-lock.json` — the way the build does.
 *
 * Usage: node scripts/check-tauri-pairs.mjs
 */
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** Every `[[package]]` in a Cargo.lock, by name — the version the build uses. */
function cargoVersions(lockPath) {
  const versions = new Map();
  for (const block of readFileSync(lockPath, "utf8").split("[[package]]")) {
    const name = block.match(/^name = "([^"]+)"$/m)?.[1];
    const version = block.match(/^version = "([^"]+)"$/m)?.[1];
    if (name && version) versions.set(name, version);
  }
  return versions;
}

const crates = cargoVersions(join(ROOT, "src-tauri", "Cargo.lock"));
const npm = JSON.parse(readFileSync(join(ROOT, "package-lock.json"), "utf8")).packages;
const minor = (v) => v.split(".").slice(0, 2).join(".");

const pairs = [["tauri", "@tauri-apps/api"]];
for (const name of Object.keys(npm)) {
  const plugin = name.match(/^node_modules\/@tauri-apps\/plugin-(.+)$/)?.[1];
  if (plugin) pairs.push([`tauri-plugin-${plugin}`, `@tauri-apps/plugin-${plugin}`]);
}

const mismatched = [];
for (const [crate, pkg] of pairs) {
  const rust = crates.get(crate);
  const js = npm[`node_modules/${pkg}`]?.version;
  if (!rust || !js) continue; // one side only: nothing for the build to compare
  if (minor(rust) !== minor(js)) mismatched.push(`${crate} ${rust} : ${pkg} ${js}`);
}

if (mismatched.length) {
  console.error("Tauri packages on different minor releases (the release build refuses these):");
  for (const line of mismatched) console.error(`  ${line}`);
  console.error("Move the npm package (or the crate) to the same minor release.");
  process.exit(1);
}
console.log(`Tauri crates and npm packages agree: ${pairs.length} pair(s) checked.`);
