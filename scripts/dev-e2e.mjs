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
import { rmSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

/**
 * Base directory the app is forced to resolve its default vault under.
 * Keep in sync with E2E_DEFAULT_VAULT_BASE in e2e/helpers.ts — the specs assert against it, so a
 * drift surfaces as a named precondition failure rather than a confusing assertion mismatch.
 */
const DEFAULT_VAULT_BASE = join(tmpdir(), "textree-e2e-default-vault");

// Only ever recreate a directory we own: inside the temp dir and under our own name.
if (resolve(DEFAULT_VAULT_BASE) !== resolve(join(tmpdir(), "textree-e2e-default-vault"))) {
  throw new Error(`Refusing to reset an unexpected path: ${DEFAULT_VAULT_BASE}`);
}
rmSync(DEFAULT_VAULT_BASE, { recursive: true, force: true });
mkdirSync(DEFAULT_VAULT_BASE, { recursive: true });

console.log(`[dev:e2e] default vault base: ${DEFAULT_VAULT_BASE}`);

const child = spawn("tauri", ["dev", "--config", "src-tauri/tauri.e2e.conf.json"], {
  stdio: "inherit",
  shell: true,
  env: { ...process.env, TEXTREE_DEFAULT_VAULT_BASE: DEFAULT_VAULT_BASE },
});

child.on("exit", (code, signal) => {
  process.exit(signal ? 1 : (code ?? 0));
});
