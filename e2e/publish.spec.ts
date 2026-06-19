import { test, expect, type Browser, type Page } from "@playwright/test";
import { mkdtempSync, existsSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
} from "./helpers";

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

/**
 * Drive the publish flow through the dev bridge (bypasses the native folder picker, mirroring
 * loadVault). Requires the app to be launched with TEXTREE_CANOPY_CLI pointing at canopy's CLI.
 */
async function publishTo(p: Page, out: string): Promise<void> {
  await p.evaluate(
    (o) =>
      (
        window as unknown as { __textreeTest: { publishTo: (o: string) => Promise<void> } }
      ).__textreeTest.publishTo(o),
    out,
  );
}

test("publish renders the vault to an auto-theming static site, source untouched", async () => {
  const source = "# Hello\r\n\r\nworld\r\n"; // CRLF: the source must come back byte-identical
  const vault = createTempVault({ "note.md": source });
  const out = mkdtempSync(join(tmpdir(), "textree-pub-")).replace(/\\/g, "/");
  try {
    await loadVault(page, vault);
    await publishTo(page, out);

    // canopy emitted the page.
    await expect
      .poll(() => existsSync(join(out, "note.html")), { timeout: 15000 })
      .toBe(true);

    // The injected tokens were rewritten for the OS preference (auto theme on the static site):
    // the dark block now lives inside a prefers-color-scheme media query wrapping :root.
    const tokens = readFileSync(join(out, "tokens.css"), "utf8");
    const mediaAt = tokens.indexOf("@media (prefers-color-scheme: dark)");
    expect(mediaAt).toBeGreaterThanOrEqual(0);
    expect(tokens.slice(mediaAt)).toContain(":root {");

    // D13: publish is read-only over the source — the note (CRLF included) is byte-unchanged.
    expect(readVaultFile(vault, "note.md")).toBe(source);

    // The UI surfaced the success notice with self-host guidance (vault-level, no note open).
    await expect(page.locator(".publish-banner.ok")).toContainText("Published");
  } finally {
    removeTempVault(vault);
    removeTempVault(out);
  }
});

test("publish into the vault is rejected with friendly, actionable guidance", async () => {
  // Publishing into the vault itself violates the read-only-outward boundary (D13). canopy is
  // resolved before the boundary check, so this (like the test above) needs TEXTREE_CANOPY_CLI.
  // The raw backend error ("the output directory must be outside the vault") is rewritten by
  // friendlyError to actionable guidance — verifies the domain string matches the mapping key.
  const vault = createTempVault({ "note.md": "# Hi\n" });
  try {
    await loadVault(page, vault);
    await publishTo(page, vault);
    await expect(page.locator(".publish-banner.error")).toContainText("outside your vault");
  } finally {
    removeTempVault(vault);
  }
});
