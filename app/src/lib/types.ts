// Types shared with the Rust backend (see app/src-tauri/src/commands.rs).

export interface AppInfo {
  name: string;
  version: string;
  platform: string;
  elevated: boolean;
}

export type DriveKind = "fixed" | "removable" | "network" | "optical" | "ramDisk" | "unknown";

export interface DriveInfo {
  root: string;
  label: string;
  fileSystem: string;
  kind: DriveKind;
  total: number;
  free: number;
  isSystem: boolean;
}

export type ScanPhase = "starting" | "walking" | "readingMft" | "building";

export interface ScanProgress {
  phase: ScanPhase;
  files: number;
  dirs: number;
  bytes: number;
  done: number;
  total: number;
  current: string | null;
}

export interface Skipped {
  path: string;
  reason: string;
}

export interface ScanSummary {
  root: string;
  method: "mft" | "walk";
  mftUnavailable: string | null;
  startedAt: number;
  durationMs: number;
  size: number;
  allocated: number;
  files: number;
  dirs: number;
  skipped: number;
  skippedDetails: Skipped[];
  volume: { total: number; free: number } | null;
}

export type ScanEvent =
  | ({ type: "progress" } & ScanProgress)
  | { type: "done"; scanId: number; summary: ScanSummary; snapshotSaved: boolean }
  | { type: "cancelled"; scanId: number }
  | { type: "failed"; scanId: number; message: string };

export interface NodeView {
  id: number;
  name: string;
  isDir: boolean;
  size: number;
  allocated: number;
  files: number;
  dirs: number;
  modified: number;
  flags: number;
  childCount: number;
}

export interface NodeDetail extends NodeView {
  path: string;
  lineage: { id: number; name: string }[];
}

export type SortKey = "size" | "allocated" | "name" | "modified" | "files";

export interface ChildrenQuery {
  offset: number;
  limit: number;
  sort: SortKey;
  descending: boolean;
  dirsOnly: boolean;
}

export interface ChildrenPage {
  total: number;
  items: NodeView[];
  restCount: number;
  restSize: number;
}

export interface TreemapNode {
  id: number;
  name: string;
  size: number;
  isDir: boolean;
  flags: number;
  children: TreemapNode[];
  otherSize: number;
  otherCount: number;
}

/** Node flag bits (mirror of `cleaner_core::tree::flags`). */
export const Flags = {
  DIR: 1 << 0,
  LINK: 1 << 1,
  HARDLINK: 1 << 2,
  HARDLINK_DUP: 1 << 3,
  HAS_HARDLINKS: 1 << 4,
  UNREADABLE: 1 << 5,
  METAFILE: 1 << 6,
  CLOUD: 1 << 7,
  HIDDEN: 1 << 8,
  SYSTEM: 1 << 9,
  HAS_UNREADABLE: 1 << 10,
} as const;
