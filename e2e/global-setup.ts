import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

/**
 * Renders a one-note folder once before the suite, so the publish specs measure publishing and
 * not the renderer's first start.
 *
 * The first render after the renderer's files were written (a fresh assembly, or the first run
 * after a reboot) reads thousands of freshly written node_modules files — measured at 23 s,
 * against about 2 s afterwards. Inside a spec that also loads a folder, that alone can use up the
 * 30 s test budget, and did (publish.spec.ts, twice). The app allows a render two minutes, so the
 * slow first start is not a failure there; here it is only noise.
 *
 * The time is printed, so a first start that grows stays visible instead of hidden.
 */
export default function globalSetup(): void {
  const cli = process.env.TEXTREE_CANOPY_CLI ?? resolve("src-tauri/resources/canopy/cli.js");
  if (!existsSync(cli)) return; // the publish specs report the missing renderer themselves

  const root = mkdtempSync(join(tmpdir(), "textree-e2e-warmup-"));
  try {
    const vault = join(root, "vault");
    mkdirSync(vault);
    writeFileSync(join(vault, "note.md"), "# Warm-up\n");
    const started = Date.now();
    const run = spawnSync("node", [cli, "build", vault, join(root, "site")], { timeout: 120_000 });
    const ms = Date.now() - started;
    if (run.status === 0) console.log(`[e2e] renderer warm-up: ${ms} ms`);
    else console.warn(`[e2e] renderer warm-up failed after ${ms} ms (status ${run.status}) — publish specs may time out`);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
