import { EditorState } from "@codemirror/state";
import { expect, test } from "vitest";
import { detectLineEnding, normalizeLineEndings } from "./eol.helpers";

test("detects CRLF when the first line break is \\r\\n", () => {
  expect(detectLineEnding("---\r\ntitle: x\r\n---\r\nbody")).toBe("\r\n");
});

test("detects LF for unix line endings", () => {
  expect(detectLineEnding("a\nb\nc")).toBe("\n");
});

test("defaults to LF for a single line with no break", () => {
  expect(detectLineEnding("no breaks here")).toBe("\n");
});

test("defaults to LF for an empty document", () => {
  expect(detectLineEnding("")).toBe("\n");
});

test("classifies by the first line break (a CRLF before any lone LF wins)", () => {
  expect(detectLineEnding("a\r\nb\nc")).toBe("\r\n");
});

test("normalizeLineEndings leaves LF text unchanged", () => {
  expect(normalizeLineEndings("a\nb\nc", "\n")).toBe("a\nb\nc");
});

test("normalizeLineEndings rewrites LF-joined text to CRLF", () => {
  expect(normalizeLineEndings("a\nb\nc", "\r\n")).toBe("a\r\nb\r\nc");
});

test("normalizeLineEndings is stable when the source already has CRLF", () => {
  expect(normalizeLineEndings("a\r\nb", "\r\n")).toBe("a\r\nb");
});

// The reason the fix exists: CodeMirror's `Text.toString()` always joins lines with `\n`,
// so a CRLF source silently becomes LF on the next edit — a byte-fidelity violation against
// a vault shared with another editor (e.g. Obsidian on Windows under git).
test("characterizes the CodeMirror behavior that loses CRLF", () => {
  const src = "a\r\nb";
  const state = EditorState.create({ doc: src });
  expect(state.doc.toString()).toBe("a\nb");
});

test("normalizing the editor output against the detected ending round-trips a CRLF source", () => {
  const src = "line one\r\nline two\r\n";
  const state = EditorState.create({ doc: src });
  const out = normalizeLineEndings(state.doc.toString(), detectLineEnding(src));
  expect(out).toBe(src);
});

test("normalizing the editor output round-trips an LF source unchanged", () => {
  const src = "line one\nline two\n";
  const state = EditorState.create({ doc: src });
  const out = normalizeLineEndings(state.doc.toString(), detectLineEnding(src));
  expect(out).toBe(src);
});
