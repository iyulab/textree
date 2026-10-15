import { describe, expect, it } from "vitest";
import cases from "./publishScope.cases.json";
import { isIncluded, type PublishChoice } from "./publishScope.helpers";

describe("isIncluded", () => {
  for (const c of cases as { why: string; choices: PublishChoice[]; rel: string; included: boolean }[]) {
    it(c.why, () => {
      expect(isIncluded(c.rel, c.choices)).toBe(c.included);
    });
  }
});

