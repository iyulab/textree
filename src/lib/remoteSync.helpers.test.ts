import { describe, expect, it } from "vitest";
import {
  exchangeMessages,
  manualOutcome,
  nameList,
  needsAttention,
  noteDisplayName,
  relativeTime,
  repositoryLabel,
} from "./remoteSync.helpers";
import type { RemoteExchange } from "./ipc";

function exchange(part: Partial<RemoteExchange>): RemoteExchange {
  return { received: [], removed: [], held: [], keptBack: [], sent: false, ...part };
}

describe("repositoryLabel", () => {
  it("keeps host and path, without scheme or .git", () => {
    expect(repositoryLabel("https://github.com/you/notes.git")).toBe("github.com/you/notes");
  });

  it("drops a sign-in part, a trailing slash and a query", () => {
    // forbidden-tokens: allow personal — the sign-in part of a URL, not anyone's mail address
    expect(repositoryLabel("https://me@gitlab.com/group/sub/notes.git/?x=1")).toBe(
      "gitlab.com/group/sub/notes",
    );
  });

  it("keeps a port and works without a path", () => {
    expect(repositoryLabel("http://127.0.0.1:8080/repo.git")).toBe("127.0.0.1:8080/repo");
    expect(repositoryLabel("https://example.com")).toBe("example.com");
  });
});

describe("noteDisplayName", () => {
  it("shows the file name without folders or .md", () => {
    expect(noteDisplayName("projects/plan.md")).toBe("plan");
    expect(noteDisplayName("a\\b\\Idea.MD")).toBe("Idea");
    expect(noteDisplayName("picture.png")).toBe("picture.png");
  });
});

describe("nameList", () => {
  it("lists up to five names", () => {
    expect(nameList(["a.md", "b.md"])).toBe("a, b");
    expect(nameList(["1.md", "2.md", "3.md", "4.md", "5.md"])).toBe("1, 2, 3, 4, 5");
  });

  it("says how many more there are past five", () => {
    expect(nameList(["1.md", "2.md", "3.md", "4.md", "5.md", "6.md", "7.md"])).toBe(
      "1, 2, 3, 4, 5 and 2 more",
    );
  });
});

describe("exchangeMessages", () => {
  it("says nothing when nothing changed", () => {
    expect(exchangeMessages(exchange({ sent: true }))).toEqual([]);
  });

  it("counts notes that arrived, singular and plural", () => {
    expect(exchangeMessages(exchange({ received: ["a.md"] }))).toEqual([
      "1 note arrived from your backup.",
    ]);
    expect(exchangeMessages(exchange({ received: ["a.md", "b.md"] }))).toEqual([
      "2 notes arrived from your backup.",
    ]);
  });

  it("says where removed notes went", () => {
    expect(exchangeMessages(exchange({ removed: ["a.md"] }))).toEqual([
      "1 note was removed there and is in Deleted notes here.",
    ]);
    expect(exchangeMessages(exchange({ removed: ["a.md", "b.md"] }))).toEqual([
      "2 notes were removed there and are in Deleted notes here.",
    ]);
  });

  it("names notes changed on both sides", () => {
    expect(exchangeMessages(exchange({ held: ["dir/plan.md"] }))).toEqual([
      "1 note changed both here and elsewhere — kept as it is for now: plan",
    ]);
    expect(exchangeMessages(exchange({ held: ["a.md", "b.md"] }))).toEqual([
      "2 notes changed both here and elsewhere — kept as they are for now: a, b",
    ]);
  });

  it("names notes whose incoming changes wait for a version", () => {
    expect(exchangeMessages(exchange({ keptBack: ["a.md"] }))).toEqual([
      "1 note has changes you haven't added as a version, so what arrived for it is waiting: a",
    ]);
    expect(exchangeMessages(exchange({ keptBack: ["a.md", "b.md"] }))).toEqual([
      "2 notes have changes you haven't added as a version, so what arrived for them is waiting: a, b",
    ]);
  });

  it("never uses machinery words", () => {
    const all = exchangeMessages(
      exchange({ received: ["a.md"], removed: ["b.md"], held: ["c.md"], keptBack: ["d.md"] }),
    ).join(" ");
    expect(all).not.toMatch(/\b(commit|branch|merge|push|pull|remote|conflict|ref)\b/i);
  });
});

describe("needsAttention", () => {
  it("is true only for notes that could not be taken in", () => {
    expect(needsAttention(exchange({ received: ["a.md"] }))).toBe(false);
    expect(needsAttention(exchange({ held: ["a.md"] }))).toBe(true);
    expect(needsAttention(exchange({ keptBack: ["a.md"] }))).toBe(true);
  });
});

describe("manualOutcome", () => {
  it("says everything is backed up when nothing needed doing", () => {
    expect(manualOutcome(exchange({ sent: true }))).toBe("Everything is backed up.");
  });

  it("joins what happened otherwise", () => {
    expect(manualOutcome(exchange({ received: ["a.md"], removed: ["b.md"] }))).toBe(
      "1 note arrived from your backup. 1 note was removed there and is in Deleted notes here.",
    );
  });
});

describe("relativeTime", () => {
  const now = 1_000_000_000;
  it("reads naturally at each scale", () => {
    expect(relativeTime(now - 5_000, now)).toBe("just now");
    expect(relativeTime(now - 60_000, now)).toBe("1 minute ago");
    expect(relativeTime(now - 5 * 60_000, now)).toBe("5 minutes ago");
    expect(relativeTime(now - 60 * 60_000, now)).toBe("1 hour ago");
    expect(relativeTime(now - 3 * 3_600_000, now)).toBe("3 hours ago");
    expect(relativeTime(now - 24 * 3_600_000, now)).toBe("1 day ago");
    expect(relativeTime(now - 72 * 3_600_000, now)).toBe("3 days ago");
  });

  it("treats a time in the future as now", () => {
    expect(relativeTime(now + 10_000, now)).toBe("just now");
  });
});
