// Vault paths — comparison and composition of the absolute paths the tree hands out, and where a
// structure change puts them. Pure: nothing here touches the disk.
//
// Paths compare the way Windows does: separators and a trailing slash are ignored, and so is case.

import type { Remap } from "./noteSave";

const normalize = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();

/** Is `child` equal to or below `ancestor`. */
export function pathInside(child: string, ancestor: string): boolean {
  const c = normalize(child);
  const a = normalize(ancestor);
  return c === a || c.startsWith(a + "/");
}

/** Do `a` and `b` name the same place. */
export function samePath(a: string, b: string): boolean {
  return normalize(a) === normalize(b);
}

/** Parent directory of a path (separator preserved). */
export function parentDir(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i >= 0 ? p.slice(0, i) : p;
}

/** Last component of a path (file or folder name). */
export function baseName(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i >= 0 ? p.slice(i + 1) : p;
}

/** Join `child` onto `dir`, keeping the separator style `dir` already uses. */
export function joinPath(dir: string, child: string): string {
  const sep = dir.includes("\\") ? "\\" : "/";
  return `${dir}${sep}${child}`;
}

/** A note's name: its file name without `.md`. */
export function noteStem(p: string): string {
  return baseName(p).replace(/\.md$/i, "");
}

/** The note a folder holds as its own body: `<dir>/<dir name>.md`. */
export function folderBody(dir: string): string {
  return joinPath(dir, `${baseName(dir)}.md`);
}

/** `from` (and everything under it) now lives at `to`. */
export function remapMoved(from: string, to: string): Remap {
  return (p) => (pathInside(p, from) ? to + p.slice(from.length) : p);
}

/**
 * `target` was renamed to `renamed`. A folder carries its own body note along under the new name,
 * so `<old>/<old>.md` becomes `<new>/<new>.md`; everything else under it keeps its name.
 */
export function remapRenamed(target: string, renamed: string, isFolder: boolean): Remap {
  const moved = remapMoved(target, renamed);
  if (!isFolder) return moved;
  const oldBody = folderBody(target);
  const newBody = folderBody(renamed);
  return (p) => (samePath(p, oldBody) ? newBody : moved(p));
}

/** The note `leaf` became a folder `dir`, and is now that folder's body note. */
export function remapPromoted(leaf: string, dir: string): Remap {
  const body = folderBody(dir);
  return (p) => (samePath(p, leaf) ? body : p);
}

/**
 * `src` was moved into `leaf`, which became a folder to hold it; `moved` is where `src` landed.
 * The leaf is now that folder's body note.
 */
export function remapAdopted(src: string, leaf: string, moved: string): Remap {
  const promoted = remapPromoted(leaf, parentDir(moved));
  const carried = remapMoved(src, moved);
  return (p) => (samePath(p, leaf) ? promoted(p) : carried(p));
}
