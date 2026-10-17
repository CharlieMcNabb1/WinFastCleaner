// Pure helpers for the Files and What grew tabs.

import { formatSize } from "./format.ts";
import type { DupeGroupView, FileKind } from "./types.ts";

export const KIND_LABELS: Record<FileKind, string> = {
  video: "Video",
  audio: "Audio",
  image: "Images",
  archive: "Archives",
  installer: "Installers",
  diskImage: "Disk images",
  document: "Documents",
  other: "Other",
};

export const KINDS = Object.keys(KIND_LABELS) as FileKind[];

/** "C:\\a\\b.txt" -> { name: "b.txt", folder: "C:\\a" } */
export function splitPath(path: string): { name: string; folder: string } {
  const i = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  if (i < 0) return { name: path, folder: "" };
  return { name: path.slice(i + 1), folder: path.slice(0, i) || path.slice(0, i + 1) };
}

/** Copies picked for removal: group index -> file indexes. */
export type DupePicks = Map<number, Set<number>>;

/** Whether `file` can be picked without picking every copy in the group. */
export function canPick(group: DupeGroupView, picks: DupePicks, file: number): boolean {
  const picked = picks.get(group.index);
  if (picked?.has(file)) return true;
  return (picked?.size ?? 0) + 1 < group.files.length;
}

/** Picks every copy except the newest (or oldest) one in each group. Never
 *  picks the last copy. */
export function pickAllButOne(groups: DupeGroupView[], keep: "newest" | "oldest"): DupePicks {
  const picks: DupePicks = new Map();
  for (const g of groups) {
    if (g.files.length < 2) continue;
    let kept = g.files[0]!;
    for (const f of g.files) {
      if (keep === "newest" ? f.modified > kept.modified : f.modified < kept.modified) kept = f;
    }
    picks.set(g.index, new Set(g.files.filter((f) => f.index !== kept.index).map((f) => f.index)));
  }
  return picks;
}

export function pickTotals(groups: DupeGroupView[], picks: DupePicks): { count: number; size: number } {
  let count = 0;
  let size = 0;
  for (const g of groups) {
    const n = picks.get(g.index)?.size ?? 0;
    count += n;
    size += n * g.size;
  }
  return { count, size };
}

/** "+3.1 GB" / "-200.0 MB" */
export function signedSize(delta: number): string {
  return `${delta >= 0 ? "+" : "-"}${formatSize(Math.abs(delta))}`;
}
