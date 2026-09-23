import { appendFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import type { FullResult, Reporter, TestCase, TestResult } from "@playwright/test/reporter";

/**
 * Appends one block per run to `e2e-results/history.log`: when, the totals, and every test that
 * failed or needed a retry — with its location and the first line of its error.
 *
 * The console output scrolls away and `test-results/` is wiped by the next run, so a failure that
 * does not come back on a re-run used to leave no name behind. The history keeps it, and across
 * runs shows which tests fail now and then.
 */
export default class RunHistoryReporter implements Reporter {
  private readonly file = resolve("e2e-results", "history.log");
  private readonly failures: string[] = [];
  private passed = 0;
  private failed = 0;
  private skipped = 0;
  private readonly started = new Date();

  onTestEnd(test: TestCase, result: TestResult): void {
    if (result.status === "skipped") {
      this.skipped += 1;
      return;
    }
    if (result.status === test.expectedStatus) {
      this.passed += 1;
      return;
    }
    this.failed += 1;
    const where = `${test.location.file.replace(/^.*[\\/]e2e[\\/]/, "e2e/")}:${test.location.line}`;
    const firstLine = (result.error?.message ?? result.status).split("\n")[0].replace(/\x1b\[[0-9;]*m/g, "");
    this.failures.push(`  ✘ ${where} › ${test.title}\n      ${firstLine} (${result.duration} ms)`);
  }

  onEnd(result: FullResult): void {
    const header = `${this.started.toISOString()}  ${result.status}  passed ${this.passed} · failed ${this.failed} · skipped ${this.skipped}`;
    try {
      mkdirSync(dirname(this.file), { recursive: true });
      appendFileSync(this.file, [header, ...this.failures, ""].join("\n"));
    } catch {
      // A history that cannot be written must not fail the run it describes.
    }
  }
}
