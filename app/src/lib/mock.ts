// Fake backend for `npm run dev:mock`: lets the UI be developed and
// screenshotted in a plain browser. All names here are synthetic. This file
// is never included in production builds.

import type { Backend } from "./backend.ts";
import type { ChildrenPage, DriveInfo, NodeDetail, NodeView, ScanSummary, TreemapNode } from "./types.ts";
import { Flags } from "./types.ts";

interface MockNode extends NodeView {
  parent: number | null;
  kids: number[];
}

const GB = 1024 ** 3;
const MB = 1024 ** 2;

function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

function buildTree(rootName: string): MockNode[] {
  const rand = rng(42);
  const nodes: MockNode[] = [];
  const add = (name: string, parent: number | null, isDir: boolean, size = 0, flags = 0): number => {
    const id = nodes.length;
    nodes.push({
      id,
      name,
      parent,
      kids: [],
      isDir,
      size,
      allocated: size,
      files: isDir ? 0 : 1,
      dirs: 0,
      modified: 1_790_000_000 - Math.floor(rand() * 90_000_000),
      flags: flags | (isDir ? Flags.DIR : 0),
      childCount: 0,
    });
    if (parent !== null) nodes[parent]!.kids.push(id);
    return id;
  };
  const files = (parent: number, n: number, ext: string[], maxSize: number) => {
    for (let i = 0; i < n; i++) {
      const e = ext[i % ext.length]!;
      add(`file-${i + 1}.${e}`, parent, false, Math.floor(rand() ** 3 * maxSize) + 1);
    }
  };

  const root = add(rootName, null, true);
  const users = add("Users", root, true);
  const user = add("test-user", users, true);
  const videos = add("Videos", user, true);
  files(videos, 12, ["mp4", "mkv", "mov"], 6 * GB);
  const downloads = add("Downloads", user, true);
  files(downloads, 40, ["zip", "exe", "iso", "pdf", "msi"], 3 * GB);
  const docs = add("Documents", user, true);
  files(docs, 60, ["pdf", "docx", "xlsx", "txt"], 40 * MB);
  const pics = add("Pictures", user, true);
  files(pics, 200, ["jpg", "png", "heic"], 12 * MB);
  const appdata = add("AppData", user, true, 0, Flags.HIDDEN);
  const local = add("Local", appdata, true);
  const cache = add("ExampleBrowser Cache", local, true);
  files(cache, 300, ["dat", "tmp"], 4 * MB);
  const dev = add("dev-projects", user, true);
  const nm = add("node_modules", dev, true);
  files(nm, 400, ["js", "json", "ts"], 2 * MB);
  add("Docs shortcut", user, true, 0, Flags.LINK);

  const win = add("Windows", root, true, 0, Flags.HAS_HARDLINKS);
  const winsxs = add("WinSxS", win, true, 0, Flags.HAS_HARDLINKS);
  files(winsxs, 120, ["dll", "sys", "cat"], 80 * MB);
  const sys32 = add("System32", win, true, 0, Flags.HAS_HARDLINKS);
  files(sys32, 120, ["dll", "exe"], 40 * MB);
  const pf = add("Program Files", root, true);
  for (const app of ["Example Office", "Sample Game", "Photo Editor"]) {
    const d = add(app, pf, true);
    files(d, 30, ["dll", "exe", "pak"], 900 * MB);
  }
  add("System Volume Information", root, true, 0, Flags.UNREADABLE | Flags.HIDDEN | Flags.SYSTEM);
  add("pagefile.sys", root, false, 16 * GB, Flags.HIDDEN | Flags.SYSTEM);
  add("hiberfil.sys", root, false, 12 * GB, Flags.HIDDEN | Flags.SYSTEM);
  add("$MFT", root, false, 1.4 * GB, Flags.METAFILE);

  // Aggregate bottom-up (children always have larger ids than parents).
  for (let i = nodes.length - 1; i > 0; i--) {
    const n = nodes[i]!;
    const p = nodes[n.parent!]!;
    p.size += n.size;
    p.allocated += n.allocated;
    p.files += n.files;
    p.dirs += n.dirs + (n.isDir ? 1 : 0);
    p.modified = Math.max(p.modified, n.modified);
    if (n.flags & (Flags.UNREADABLE | Flags.HAS_UNREADABLE)) p.flags |= Flags.HAS_UNREADABLE;
  }
  for (const n of nodes) {
    n.kids.sort((a, b) => nodes[b]!.size - nodes[a]!.size);
    n.childCount = n.kids.length;
  }
  return nodes;
}

function view(n: MockNode): NodeView {
  const { parent: _p, kids: _k, ...rest } = n;
  return rest;
}

export function createMockBackend(): Backend {
  let nodes: MockNode[] = [];
  let scanId = 0;
  let timer: ReturnType<typeof setInterval> | null = null;
  const drives: DriveInfo[] = [
    { root: "C:\\", label: "Windows", fileSystem: "NTFS", kind: "fixed", total: 476 * GB, free: 61 * GB, isSystem: true },
    { root: "D:\\", label: "Data", fileSystem: "NTFS", kind: "fixed", total: 1863 * GB, free: 1210 * GB, isSystem: false },
    { root: "E:\\", label: "USB", fileSystem: "exFAT", kind: "removable", total: 58 * GB, free: 40 * GB, isSystem: false },
  ];
  const get = (id: number): MockNode => {
    const n = nodes[id];
    if (!n) throw new Error("Unknown item.");
    return n;
  };
  const pathOf = (id: number): string => {
    const parts: string[] = [];
    let cur: MockNode | undefined = get(id);
    while (cur && cur.parent !== null) {
      parts.unshift(cur.name);
      cur = nodes[cur.parent];
    }
    return nodes[0]!.name + parts.join("\\");
  };
  const treemap = (id: number, depth: number, max: number): TreemapNode => {
    const n = get(id);
    const kids = depth > 0 ? n.kids.filter((k) => get(k).size > 0) : [];
    return {
      id: n.id,
      name: n.name,
      size: n.size,
      isDir: n.isDir,
      flags: n.flags,
      children: kids.slice(0, max).map((k) => treemap(k, depth - 1, max)),
      otherSize: kids.slice(max).reduce((s, k) => s + get(k).size, 0),
      otherCount: Math.max(0, kids.length - max),
    };
  };

  return {
    appInfo: async () => ({ name: "WinFastCleaner", version: "0.1.0 (mock)", platform: "windows", elevated: false }),
    listDrives: async () => drives,
    startScan: async (path, allowMft, onEvent) => {
      if (timer) throw new Error("A scan is already running.");
      scanId += 1;
      const id = scanId;
      let step = 0;
      timer = setInterval(() => {
        step += 1;
        onEvent({
          type: "progress",
          phase: "walking",
          files: step * 23_456,
          dirs: step * 1_234,
          bytes: step * 7.3 * GB,
          done: 0,
          total: 0,
          current: `${path}Users\\test-user\\AppData\\Local\\folder-${step}`,
        });
        if (step >= 12) {
          clearInterval(timer!);
          timer = null;
          nodes = buildTree(path);
          const r = nodes[0]!;
          const drive = drives.find((d) => d.root === path);
          const summary: ScanSummary = {
            root: path,
            method: allowMft ? "walk" : "walk",
            mftUnavailable: "Fast MFT scan needs administrator rights",
            startedAt: Math.floor(Date.now() / 1000),
            durationMs: 1800,
            size: r.size,
            allocated: r.allocated,
            files: r.files,
            dirs: r.dirs,
            skipped: 1,
            skippedDetails: [{ path: `${path}System Volume Information`, reason: "Access is denied. (os error 5)" }],
            volume: drive ? { total: drive.total, free: drive.free } : null,
          };
          onEvent({ type: "done", scanId: id, summary, snapshotSaved: true });
        }
      }, 120);
      return id;
    },
    cancelScan: async () => {
      if (timer) {
        clearInterval(timer);
        timer = null;
      }
    },
    getNode: async (_scan, id): Promise<NodeDetail> => {
      const lineage: { id: number; name: string }[] = [];
      let cur: MockNode | undefined = get(id);
      while (cur) {
        lineage.unshift({ id: cur.id, name: cur.name });
        cur = cur.parent === null ? undefined : nodes[cur.parent];
      }
      return { ...view(get(id)), path: pathOf(id), lineage };
    },
    listChildren: async (_scan, id, q): Promise<ChildrenPage> => {
      let kids = get(id).kids.map(get).filter((k) => !q.dirsOnly || k.isDir);
      const key: Record<string, (n: MockNode) => number | string> = {
        size: (n) => n.size,
        allocated: (n) => n.allocated,
        name: (n) => n.name.toLowerCase(),
        modified: (n) => n.modified,
        files: (n) => n.files,
      };
      const f = key[q.sort]!;
      kids = [...kids].sort((a, b) => (f(a) < f(b) ? -1 : f(a) > f(b) ? 1 : 0));
      if (q.descending) kids.reverse();
      const page = kids.slice(q.offset, q.offset + q.limit);
      const rest = kids.slice(q.offset + q.limit);
      return {
        total: kids.length,
        items: page.map(view),
        restCount: rest.length,
        restSize: rest.reduce((s, n) => s + n.size, 0),
      };
    },
    treemap: async (_scan, id, depth, max) => treemap(id, depth, max),
    openInExplorer: async () => {},
    relaunchElevated: async () => {
      throw new Error("Restarting as administrator is not available in the mock.");
    },
    pickFolder: async () => "D:\\",
  };
}
