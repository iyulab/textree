/*
 * Runs the E2E suite end to end: clears what an earlier session left running, starts the app
 * through the E2E launcher, waits until the app page is reachable over CDP, runs the specs, and
 * stops the app again — whatever happened in between.
 *
 * Doing this by hand has two ways to go wrong that the specs cannot detect: an app from an earlier
 * build still answering on the CDP port (the specs then pass or fail against old code), and a vite
 * left on the dev port after the app was stopped (the next launch fails to bind it).
 *
 * Usage:
 *   npm run e2e:run                          whole suite once
 *   npm run e2e:run -- --soak 3              whole suite three times against one app instance
 *   npm run e2e:run -- e2e/publish.spec.ts   anything else is passed to `playwright test`
 *
 * The app's own output goes to e2e-results/app.log; its tail is printed when startup fails.
 */

import { spawn, spawnSync } from "node:child_process";
import { closeSync, existsSync, mkdirSync, openSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { checkCanopy, describeCanopy } from "./sidecar-provenance.mjs";
import { devServerListeners, killTree, runningDevApps } from "./dev-processes.mjs";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");
/** Written by scripts/dev-e2e.mjs once it has chosen the CDP port. */
const HANDSHAKE_FILE = join(tmpdir(), "textree-e2e-cdp.json");
const APP_URL_FRAGMENT = "localhost:1420";
/** The first launch compiles the Rust side, which takes minutes. */
const STARTUP_TIMEOUT_MS = 10 * 60_000;
const APP_LOG = join(REPO, "e2e-results", "app.log");
const ASSEMBLED_CANOPY = join(REPO, "src-tauri", "resources", "canopy", "cli.js");

if (process.platform !== "win32") {
  console.error("[e2e:run] the E2E suite drives WebView2 over CDP and runs on Windows only");
  process.exit(2);
}

function parseArgs(argv) {
  const rest = [];
  let soak = 1;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--soak") {
      soak = Number(argv[++i]);
      if (!Number.isInteger(soak) || soak < 1) {
        console.error(`[e2e:run] --soak needs a positive whole number, got "${argv[i]}"`);
        process.exit(2);
      }
    } else {
      rest.push(argv[i]);
    }
  }
  return { soak, playwrightArgs: rest };
}

function clearLeftovers() {
  const stale = [...runningDevApps(REPO), ...devServerListeners()];
  for (const p of stale) {
    const what = p.path ?? `listener on port 1420`;
    if (killTree(p.pid)) console.log(`[e2e:run] stopped a leftover process: pid ${p.pid} (${what})`);
  }
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

async function appPageUp(endpoint) {
  try {
    const res = await fetch(`${endpoint}/json/list`, { signal: AbortSignal.timeout(2000) });
    const targets = await res.json();
    return targets.some((t) => typeof t.url === "string" && t.url.includes(APP_URL_FRAGMENT));
  } catch {
    return false;
  }
}

function tail(file, lines) {
  if (!existsSync(file)) return "";
  return readFileSync(file, "utf8").split(/\r?\n/).slice(-lines).join("\n");
}

async function waitForApp(child) {
  const deadline = Date.now() + STARTUP_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`the app exited during startup (code ${child.exitCode})`);
    // The handshake is removed before launch, so one that exists now was written by this launch.
    if (existsSync(HANDSHAKE_FILE)) {
      const { endpoint } = JSON.parse(readFileSync(HANDSHAKE_FILE, "utf8"));
      if (await appPageUp(endpoint)) return endpoint;
    }
    await sleep(2000);
  }
  throw new Error(`the app page did not appear within ${STARTUP_TIMEOUT_MS / 60_000} minutes`);
}

const { soak, playwrightArgs } = parseArgs(process.argv.slice(2));

const env = { ...process.env };
if (!env.TEXTREE_CANOPY_CLI && existsSync(ASSEMBLED_CANOPY)) {
  // The assembled renderer, packaged the way a release packages it; publish specs need it by
  // absolute path. Whether it is also the canopy commit a release ships depends on what it was
  // assembled from — said up front, so a green run is not read as a check of the shipped renderer
  // when it was not one.
  env.TEXTREE_CANOPY_CLI = ASSEMBLED_CANOPY;
  const renderer = checkCanopy();
  const line = `[e2e:run] publishing renders with ${describeCanopy(renderer)}`;
  if (renderer.verdict === "pinned") console.log(line);
  else console.warn(line);
}
if (!env.TEXTREE_CANOPY_CLI) {
  console.warn("[e2e:run] warning: TEXTREE_CANOPY_CLI is unset and no assembled renderer exists — publish specs will fail");
}

clearLeftovers();
rmSync(HANDSHAKE_FILE, { force: true });
mkdirSync(dirname(APP_LOG), { recursive: true });
// The app writes straight to the file, not through a pipe this process drains: the specs run under
// spawnSync, which holds this event loop, and an undrained pipe fills and then blocks the app on its
// own log writes — the suite then fails from the middle on with the page gone.
const log = openSync(APP_LOG, "w");
console.log(`[e2e:run] starting the app (output: ${APP_LOG})`);
const app = spawn(process.execPath, [join(REPO, "scripts", "dev-e2e.mjs")], { cwd: REPO, env, stdio: ["ignore", log, log] });

let stopped = false;
function stopApp() {
  if (stopped) return;
  stopped = true;
  killTree(app.pid);
  // tauri dev starts vite and the app as siblings of its own shell; catch whatever escaped the tree.
  clearLeftovers();
}
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    stopApp();
    process.exit(130);
  });
}

let exitCode = 0;
try {
  const started = Date.now();
  const endpoint = await waitForApp(app);
  console.log(`[e2e:run] app is up at ${endpoint} after ${Math.round((Date.now() - started) / 1000)}s`);

  const outcomes = [];
  for (let round = 1; round <= soak; round++) {
    if (soak > 1) console.log(`\n[e2e:run] ── round ${round}/${soak}`);
    const run = spawnSync(`npx playwright test ${playwrightArgs.join(" ")}`, {
      cwd: REPO,
      env,
      stdio: "inherit",
      shell: true,
    });
    outcomes.push(run.status === 0);
  }
  const failed = outcomes.filter((ok) => !ok).length;
  if (soak > 1) console.log(`\n[e2e:run] soak: ${soak - failed}/${soak} rounds green`);
  exitCode = failed === 0 ? 0 : 1;
} catch (err) {
  console.error(`[e2e:run] ${err.message}\n--- last lines of ${APP_LOG} ---\n${tail(APP_LOG, 40)}`);
  exitCode = 1;
} finally {
  stopApp();
  closeSync(log);
}
process.exit(exitCode);
