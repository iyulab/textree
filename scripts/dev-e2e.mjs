/*
 * Launcher for the E2E build of the app.
 *
 * Its job is to establish the preconditions the specs rely on, so they cannot silently run
 * against the developer's real notes. The default-vault flow (see e2e/onboarding.spec.ts) opens
 * whatever folder the app resolves as its default home; without a forced base that is the real
 * Documents folder, and the specs then assert against personal content — they fail, or worse,
 * pass for the wrong reason. A precondition that lives only in a comment is not a precondition.
 *
 * The base directory is recreated on every launch so first-run behaviour is actually first-run.
 *
 * Usage: npm run dev:e2e
 */

import { spawn } from "node:child_process";
import { rmSync, mkdirSync, writeFileSync, readFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/**
 * Contents of the shared read-only vault the specs open with `sampleVaultPath()`.
 *
 * It is seeded here rather than kept in the repository because several specs assert against these
 * exact names: a fixture that has to exist but is not created by anything is a precondition in
 * name only, and a fresh clone would fail on it. Regenerating also means a spec run cannot inherit
 * edits a previous manual session left behind.
 */
const SAMPLE_VAULT = {
  "project.md":
    "---\ntitle: project\nicon: \u{1F4C1}\n---\n\n# project\n\n" +
    "A sample note. Follow [[journal]], or see [[library/memo]].\n",
  "journal/journal.md": "# journal\n\nA folder note. Dated notes live underneath it.\n",
  "journal/2026-06-13.md": "# 2026-06-13\n\nToday's entry.\n",
  "library/memo.md": "# memo\n\nA note in the library folder.\n",
};

/**
 * Base directory the app is forced to resolve its default vault under.
 * Keep in sync with E2E_DEFAULT_VAULT_BASE in e2e/helpers.ts — the specs assert against it, so a
 * drift surfaces as a named precondition failure rather than a confusing assertion mismatch.
 */
const DEFAULT_VAULT_BASE = join(tmpdir(), "textree-e2e-default-vault");

/**
 * Base directory the app is forced to keep per-folder settings under.
 * Keep in sync with E2E_PERSONAL_BASE in e2e/helpers.ts.
 *
 * Without this the app resolves the home directory, so a suite run writes a settings folder for
 * every temporary vault it creates into the home directory of whoever ran it, and leaves them
 * there: the vaults are removed at the end of each test, the settings keyed to them are not.
 * Forcing a base the launcher owns keeps the run self-contained and lets it start from a clean
 * slate, the same way the default-vault base does.
 */
const PERSONAL_BASE = join(tmpdir(), "textree-e2e-personal");

// Only ever recreate a directory we own: inside the temp dir and under our own name.
for (const [owned, expected] of [
  [DEFAULT_VAULT_BASE, join(tmpdir(), "textree-e2e-default-vault")],
  [PERSONAL_BASE, join(tmpdir(), "textree-e2e-personal")],
]) {
  if (resolve(owned) !== resolve(expected)) {
    throw new Error(`Refusing to reset an unexpected path: ${owned}`);
  }
  rmSync(owned, { recursive: true, force: true });
  mkdirSync(owned, { recursive: true });
}

const sampleVault = join(REPO, "sample-vault");
rmSync(sampleVault, { recursive: true, force: true });
for (const [rel, content] of Object.entries(SAMPLE_VAULT)) {
  const target = join(sampleVault, rel);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, content, "utf8");
}

console.log(`[dev:e2e] default vault base: ${DEFAULT_VAULT_BASE}`);
console.log(`[dev:e2e] personal settings base: ${PERSONAL_BASE}`);
console.log(`[dev:e2e] sample vault seeded: ${sampleVault}`);

/**
 * The CDP port is decided here and nowhere else. The overlay config keeps 9222 as its default, the
 * launcher rewrites it into a generated copy, and the chosen endpoint is written to a handshake file
 * that e2e/helpers.ts reads — so the app and the specs cannot disagree about where to meet.
 *
 * 9222 is a popular default: anything already debugging on it (a browser, an Electron app) takes
 * it, and the app then comes up with no CDP at all. With no port requested explicitly, an occupied
 * 9222 falls back to a free port; an explicitly requested port that is taken fails right here,
 * naming the occupant, instead of surfacing later as "could not find the app page".
 */
const DEFAULT_CDP_PORT = 9222;
const HANDSHAKE_FILE = join(tmpdir(), "textree-e2e-cdp.json");
const OVERLAY_TEMPLATE = join(REPO, "src-tauri", "tauri.e2e.conf.json");
const GENERATED_OVERLAY = join(tmpdir(), "textree-e2e.conf.json");

function isFree(port) {
  return new Promise((done) => {
    const server = createServer();
    server.once("error", () => done(false));
    server.listen(port, "127.0.0.1", () => server.close(() => done(true)));
  });
}

function freePort() {
  return new Promise((done, fail) => {
    const server = createServer();
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => done(port));
    });
  });
}

async function occupant(port) {
  try {
    const res = await fetch(`http://127.0.0.1:${port}/json/version`, { signal: AbortSignal.timeout(1000) });
    const info = await res.json();
    return info["User-Agent"] ?? info.Browser ?? "unknown";
  } catch {
    return "a process that is not a CDP endpoint";
  }
}

async function chooseCdpPort() {
  const requested = process.env.TEXTREE_E2E_CDP_PORT;
  if (requested !== undefined) {
    const port = Number(requested);
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      throw new Error(`TEXTREE_E2E_CDP_PORT must be a port number, got "${requested}"`);
    }
    if (!(await isFree(port))) {
      throw new Error(`CDP port ${port} (TEXTREE_E2E_CDP_PORT) is already in use by: ${await occupant(port)}`);
    }
    return port;
  }
  if (await isFree(DEFAULT_CDP_PORT)) return DEFAULT_CDP_PORT;
  const port = await freePort();
  console.log(`[dev:e2e] CDP port ${DEFAULT_CDP_PORT} is in use by: ${await occupant(DEFAULT_CDP_PORT)}`);
  console.log(`[dev:e2e] using free port ${port} instead`);
  return port;
}

const cdpPort = await chooseCdpPort();
const template = readFileSync(OVERLAY_TEMPLATE, "utf8");
if (!/--remote-debugging-port=\d+/.test(template)) {
  throw new Error(`${OVERLAY_TEMPLATE} no longer carries --remote-debugging-port; the launcher cannot set the port`);
}
writeFileSync(GENERATED_OVERLAY, template.replace(/--remote-debugging-port=\d+/, `--remote-debugging-port=${cdpPort}`), "utf8");
writeFileSync(HANDSHAKE_FILE, JSON.stringify({ endpoint: `http://localhost:${cdpPort}` }), "utf8");
console.log(`[dev:e2e] CDP endpoint: http://localhost:${cdpPort} (handshake: ${HANDSHAKE_FILE})`);

const child = spawn("tauri", ["dev", "--config", `"${GENERATED_OVERLAY}"`], {
  stdio: "inherit",
  shell: true,
  env: {
    ...process.env,
    TEXTREE_DEFAULT_VAULT_BASE: DEFAULT_VAULT_BASE,
    TEXTREE_PERSONAL_BASE: PERSONAL_BASE,
  },
});

child.on("exit", (code, signal) => {
  process.exit(signal ? 1 : (code ?? 0));
});
