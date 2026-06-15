import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
  listVaultDir,
} from "./helpers";

let browser: Browser;
let page: Page;

// 1x1 transparent PNG.
const PNG_1x1_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

/**
 * Inject an image paste event into the editor. Chromium ignores the
 * ClipboardEvent constructor's clipboardData, so we attach a DataTransfer via
 * Object.defineProperty (matching the app's contract where handlePaste reads
 * event.clipboardData.items).
 */
async function pasteImage(mime: string, base64: string): Promise<void> {
  await page.evaluate(
    ({ mime, base64 }) => {
      const bin = atob(base64);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      const file = new File([bytes], "clip", { type: mime });
      const dt = new DataTransfer();
      dt.items.add(file);
      const ev = new Event("paste", { bubbles: true, cancelable: true });
      Object.defineProperty(ev, "clipboardData", { value: dt });
      document.querySelector(".cm-content")!.dispatchEvent(ev);
    },
    { mime, base64 },
  );
}

test("paste image → save to assets/ + auto-insert link into body", async () => {
  const vault = createTempVault({ "그림노트.md": "본문\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /그림노트/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");

    await pasteImage("image/png", PNG_1x1_BASE64);

    // Markdown link is inserted into the body and persisted to disk via autosave.
    await expect
      .poll(() => readVaultFile(vault, "그림노트.md"), { timeout: 5000 })
      .toContain("![](assets/");
    // Actual image file saved to assets/ next to the note.
    await expect
      .poll(() => listVaultDir(vault, "assets").filter((f) => f.endsWith(".png")).length, {
        timeout: 5000,
      })
      .toBeGreaterThan(0);
  } finally {
    removeTempVault(vault);
  }
});

test("paste of unsupported format (image/tiff) is not intercepted — no assets created", async () => {
  const vault = createTempVault({ "무관노트.md": "본문\n" });
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /무관노트/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();
    await page.locator(".cm-content").click();

    // tiff is outside the whitelist → handlePaste lets it through (no swallowing). No attachment saved.
    await pasteImage("image/tiff", PNG_1x1_BASE64);

    // After a brief wait, confirm no assets created and no link inserted.
    await page.waitForTimeout(800);
    expect(listVaultDir(vault, "assets")).toHaveLength(0);
    expect(readVaultFile(vault, "무관노트.md")).not.toContain("![](assets/");
  } finally {
    removeTempVault(vault);
  }
});
