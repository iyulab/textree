import { describe, it, expect } from "vitest";
import { cloudPublishNotice, cloudPublishErrorNotice } from "./cloudPublish.helpers";

describe("cloudPublish.helpers", () => {
  it("formats a success notice with the url and page count", () => {
    const n = cloudPublishNotice({ url: "https://pub.textree.me", pageCount: 4 });
    expect(n.kind).toBe("ok");
    expect(n.text).toContain("https://pub.textree.me");
    expect(n.text).toContain("4");
  });

  it("formats a singular page count without an 's'", () => {
    const n = cloudPublishNotice({ url: "https://pub.textree.me", pageCount: 1 });
    expect(n.text).toContain("1 page");
    expect(n.text).not.toContain("1 pages");
  });

  it("maps an error to a friendly notice", () => {
    const n = cloudPublishErrorNotice("no publish token is set — add one in Settings");
    expect(n.kind).toBe("error");
    expect(n.text).toContain("Settings");
  });
});
