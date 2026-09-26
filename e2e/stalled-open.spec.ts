import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  stallOpening,
  releaseOpening,
} from "./helpers";

// A folder that does not answer when opened — a disconnected network or sync drive. The window must
// not wait on it, and opening another folder instead is how the person gets out.

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  releaseOpening();
  await browser?.close();
});

test("a folder that does not answer when opened: another one can be opened meanwhile, and it stays", async () => {
  const slow = createTempVault({ "slow-note.md": "# slow\n" });
  const quick = createTempVault({ "quick-note.md": "# quick\n" });
  try {
    stallOpening(slow);
    // Not awaited: this opening hangs until released.
    const hung = page.evaluate(
      (v) =>
        (window as unknown as { __textreeTest: { loadVault: (p: string) => Promise<boolean> } }).__textreeTest.loadVault(v),
      slow,
    );
    await page.waitForTimeout(500);

    // With the opening held on the main thread this would wait for it — and the test would never
    // release it, so it would time out here.
    await loadVault(page, quick);
    await expect(page.getByRole("treeitem", { name: /quick-note/ })).toBeVisible();

    releaseOpening();
    expect(await hung, "the overtaken opening reports that it did not switch").toBe(false);
    // Nothing of the slow folder arrives after the fact.
    await page.waitForTimeout(500);
    await expect(page.getByRole("treeitem", { name: /quick-note/ })).toBeVisible();
    await expect(page.getByRole("treeitem", { name: /slow-note/ })).toHaveCount(0);
  } finally {
    releaseOpening();
    removeTempVault(slow);
    removeTempVault(quick);
  }
});
