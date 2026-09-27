/**
 * Where the bundled publishing renderer lives, and which release of it the app ships.
 *
 * The renderer is the npm package @iyulab/canopy, installed as-is from the registry by the
 * manifest in canopy-sidecar/ (package.json + package-lock.json). That manifest is the single pin:
 * CI, the release build and local assembly all install from it. It also holds KaTeX to the version
 * the editor renders math with, so a published page and the editor draw the same math.
 */
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export const CANOPY_PACKAGE = "@iyulab/canopy";
/** The committed manifest the stage is installed from. */
export const CANOPY_MANIFEST_DIR = join(REPO, "canopy-sidecar");
/** The assembled payload, bundled by Tauri as the `canopy/` resource. */
export const CANOPY_STAGE = join(REPO, "src-tauri", "resources", "canopy");
/** The renderer's command-line entry inside the stage (the package's `bin`). */
export const CANOPY_CLI = join(CANOPY_STAGE, "node_modules", "@iyulab", "canopy", "dist", "cli.js");

const EXACT = /^\d+\.\d+\.\d+$/;

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

/**
 * The exact canopy version the app ships. Throws unless the manifest pins one exact release — a
 * range would let two builds of the same commit ship different renderers.
 */
export function canopyPin() {
  const manifest = readJson(join(CANOPY_MANIFEST_DIR, "package.json"));
  const pin = manifest.dependencies?.[CANOPY_PACKAGE];
  if (typeof pin !== "string" || !EXACT.test(pin)) {
    throw new Error(`canopy-sidecar/package.json must pin ${CANOPY_PACKAGE} to one exact version, got ${JSON.stringify(pin)}`);
  }
  return pin;
}

/**
 * The KaTeX versions the editor and the renderer use. Both must be exact and equal: the editor
 * shows math the way a published page will.
 */
export function katexPins() {
  return {
    editor: readJson(join(REPO, "package.json")).dependencies?.katex ?? null,
    renderer: readJson(join(CANOPY_MANIFEST_DIR, "package.json")).overrides?.katex ?? null,
  };
}

/** Problems with the pins, as sentences; empty when there are none. */
export function pinProblems() {
  const problems = [];
  try {
    canopyPin();
  } catch (e) {
    problems.push(e.message);
  }
  const { editor, renderer } = katexPins();
  if (!EXACT.test(editor ?? "")) problems.push(`package.json must pin katex to one exact version, got ${JSON.stringify(editor)}`);
  if (!EXACT.test(renderer ?? "")) {
    problems.push(`canopy-sidecar/package.json must hold katex to one exact version in "overrides", got ${JSON.stringify(renderer)}`);
  }
  if (editor && renderer && editor !== renderer) {
    problems.push(`KaTeX differs: the editor uses ${editor} (package.json), published pages ${renderer} (canopy-sidecar/package.json)`);
  }
  return problems;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const problems = pinProblems();
  if (problems.length > 0) {
    for (const p of problems) console.error(`[canopy] ${p}`);
    process.exit(1);
  }
  console.log(`[canopy] ${CANOPY_PACKAGE} ${canopyPin()}, KaTeX ${katexPins().editor} in both the editor and the renderer`);
}
