import { describe, expect, it } from "vitest";
import { parseCalloutHeader } from "./callout.helpers";

describe("parseCalloutHeader", () => {
  it("recognizes a bare core marker with a fallback title", () => {
    expect(parseCalloutHeader("[!note]")).toEqual({
      core: "note", rawType: "note", title: "Note", explicitTitle: false, markerLength: 7,
    });
  });

  it("is case-insensitive and preserves the typed word in the title", () => {
    const h = parseCalloutHeader("[!NOTE]");
    expect(h?.core).toBe("note");
    expect(h?.title).toBe("NOTE");
  });

  it("maps aliases onto core styles but titles the typed word", () => {
    const h = parseCalloutHeader("[!info]");
    expect(h?.core).toBe("note");
    expect(h?.title).toBe("Info");
  });

  it("falls back to note for unknown types", () => {
    const h = parseCalloutHeader("[!custom]");
    expect(h?.core).toBe("note");
    expect(h?.title).toBe("Custom");
  });

  it("takes an explicit title verbatim (trimmed)", () => {
    const h = parseCalloutHeader("[!tip] Pro move  ");
    expect(h).toMatchObject({ core: "tip", title: "Pro move", explicitTitle: true });
  });

  it("accepts fold suffixes and counts them in markerLength", () => {
    expect(parseCalloutHeader("[!note]- Folded")).toMatchObject({
      core: "note", title: "Folded", markerLength: 8,
    });
    expect(parseCalloutHeader("[!note]+")).toMatchObject({ title: "Note", markerLength: 8 });
  });

  it("rejects non-callout lines", () => {
    expect(parseCalloutHeader("plain text")).toBeNull();
    expect(parseCalloutHeader("[!foo-bar]")).toBeNull(); // non-alpha type
    expect(parseCalloutHeader("[!note]x")).toBeNull(); // junk right after the marker
    expect(parseCalloutHeader(" [!note]")).toBeNull(); // must open the line
    expect(parseCalloutHeader("intro [!note]")).toBeNull();
  });
});
