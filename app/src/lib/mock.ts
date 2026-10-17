// Fake backend for `npm run dev:mock`: lets the UI be developed and
// screenshotted in a plain browser. All names here are synthetic. This file
// is never included in production builds.

import type { Backend } from "./backend.ts";
import type {
  Batch,
  BinItem,
  ChildrenPage,
  CleanReport,
  ComparisonView,
  DriveInfo,
  DupeGroupView,
  FileKind,
  FoundFile,
  JunkItem,
  NodeDetail,
  NodeView,
  PlanView,
  RuleView,
  ScanSummary,
  SettingsView,
  Tier,
  TreemapNode,
} from "./types.ts";
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
      protected: false,
      excluded: false,
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
  for (const id of [
    add("System Volume Information", root, true, 0, Flags.UNREADABLE | Flags.HIDDEN | Flags.SYSTEM),
    add("pagefile.sys", root, false, 16 * GB, Flags.HIDDEN | Flags.SYSTEM),
    add("hiberfil.sys", root, false, 12 * GB, Flags.HIDDEN | Flags.SYSTEM),
    add("$MFT", root, false, 1.4 * GB, Flags.METAFILE),
  ]) {
    nodes[id]!.protected = true;
  }

  // Like the real guard: the drive root, Users, and profile folders can't be
  // removed themselves; Windows and Program Files are protected throughout.
  for (const id of [root, users, user]) nodes[id]!.protected = true;
  const protectTree = (id: number) => {
    nodes[id]!.protected = true;
    nodes[id]!.kids.forEach(protectTree);
  };
  protectTree(win);
  protectTree(pf);

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

function mockRules(): RuleView[] {
  const r = (
    id: string,
    name: string,
    group: string,
    tier: Tier,
    size: number,
    files: number,
    extra: Partial<RuleView> = {},
  ): RuleView => ({
    id,
    name,
    group,
    tier,
    description: `${name}: files the app recreates on its own when needed.`,
    lose: tier === "green" ? "Nothing; it is rebuilt automatically." : "It is downloaded or rebuilt again, which takes time.",
    requiresAdmin: false,
    closeApps: [],
    minAgeDays: null,
    userRule: false,
    status: { kind: files > 0 ? "ready" : "nothingFound" },
    files,
    size,
    hidden: 0,
    ...extra,
  });
  return [
    r("windows.user-temp", "Temporary files (your account)", "Windows", "green", 2.3 * GB, 4812, { hidden: 3 }),
    r("windows.system-temp", "Temporary files (Windows)", "Windows", "green", 640 * MB, 512, {
      requiresAdmin: true,
      status: { kind: "needsAdmin" },
    }),
    r("windows.thumbnail-cache", "Thumbnail cache", "Windows", "green", 210 * MB, 14),
    r("windows.error-reports", "Error reports (your account)", "Windows", "green", 48 * MB, 31),
    r("windows.directx-shader-cache", "DirectX shader cache", "Windows", "yellow", 1.1 * GB, 2210),
    r("windows.update-downloads", "Windows Update downloads", "Windows", "yellow", 3.4 * GB, 96, {
      requiresAdmin: true,
      minAgeDays: 10,
      status: { kind: "needsAdmin" },
    }),
    r("chrome.cache", "Google Chrome cache", "Google Chrome", "green", 1.6 * GB, 9120, {
      closeApps: ["chrome.exe"],
      status: { kind: "appRunning", detail: ["chrome.exe"] },
    }),
    r("edge.cache", "Microsoft Edge cache", "Microsoft Edge", "green", 820 * MB, 4410, { closeApps: ["msedge.exe"] }),
    r("discord.cache", "Discord cache", "Apps", "green", 3.1 * GB, 1203, { closeApps: ["Discord.exe"] }),
    r("spotify.cache", "Spotify cache", "Apps", "yellow", 2.2 * GB, 610),
    r("dev.npm-cache", "npm cache", "Developer tools", "yellow", 1.9 * GB, 30210),
    r("dev.cargo-registry-cache", "Cargo download cache", "Developer tools", "yellow", 0, 0),
  ];
}

function mockItems(rule: RuleView): JunkItem[] {
  const rand = rng(rule.id.length * 7919);
  const n = Math.min(rule.files, 400);
  const weights = Array.from({ length: n }, () => rand() ** 3 + 0.001);
  const total = weights.reduce((a, b) => a + b, 0);
  return weights
    .map((w, i) => ({
      index: i,
      path: `C:\\Users\\test-user\\AppData\\Local\\${rule.group.replace(/ /g, "")}\\Cache\\entry-${i + 1}.tmp`,
      size: Math.round((w / total) * rule.size),
      modified: 1_790_000_000 - Math.floor(rand() * 9_000_000),
    }))
    .sort((a, b) => b.size - a.size)
    .map((item, i) => ({ ...item, index: i }));
}

function mockFound(root: string, old: boolean): FoundFile[] {
  const rand = rng(old ? 11 : 7);
  const names: [string, FileKind][] = [
    ["Videos\\holiday-2019.mkv", "video"],
    ["Downloads\\win11-23h2.iso", "diskImage"],
    ["Downloads\\game-setup.exe", "installer"],
    ["Videos\\screen-recording.mp4", "video"],
    ["Downloads\\photos-backup.zip", "archive"],
    ["Documents\\scans.pdf", "document"],
    ["Downloads\\driver-pack.msi", "installer"],
    ["Music\\live-set.flac", "audio"],
    ["Pictures\\raw\\IMG_0001.cr2", "image"],
    ["Downloads\\vm-disk.vhdx", "diskImage"],
  ];
  return names
    .map(([rel, kind], i) => {
      const modified = old ? 1_700_000_000 - Math.floor(rand() * 60_000_000) : 1_790_000_000 - Math.floor(rand() * 20_000_000);
      return {
        index: i,
        path: `${root}Users\\test-user\\${rel}`,
        size: Math.floor((rand() * 6 + (old ? 0.01 : 0.1)) * GB),
        modified,
        lastUsed: modified + 3600,
        kind,
      };
    })
    .sort((a, b) => b.size - a.size)
    .map((f, i) => ({ ...f, index: i }));
}

function mockDupes(root: string): DupeGroupView[] {
  const groups: [number, string[], number[]][] = [
    [2.4 * GB, ["Downloads\\ubuntu.iso", "Desktop\\ubuntu (1).iso", "ISOs\\ubuntu.iso"], [1_780_000_000, 1_785_000_000, 1_760_000_000]],
    [850 * MB, ["Videos\\wedding.mp4", "Backup\\Videos\\wedding.mp4"], [1_700_000_000, 1_720_000_000]],
    [12 * MB, ["Documents\\report-final.pdf", "Downloads\\report-final.pdf"], [1_789_000_000, 1_789_500_000]],
  ];
  return groups.map(([size, rels, mods], gi) => ({
    index: gi,
    size,
    hash: `b3${gi}f0c0ffee${gi}`.padEnd(64, "0"),
    wasted: size * (rels.length - 1),
    files: rels.map((rel, fi) => ({ index: fi, path: `${root}Users\\test-user\\${rel}`, modified: mods[fi]! })),
  }));
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

  // Cleaner, recovery bin, and settings state.
  let junkId = 1000;
  const items = new Map<string, JunkItem[]>();
  // Keep each rule's totals equal to its (synthetic) files.
  let rules: RuleView[] = mockRules().map((r) => {
    const list = mockItems(r);
    items.set(r.id, list);
    return { ...r, files: list.length, size: list.reduce((s, i) => s + i.size, 0) };
  });
  let plan: (PlanView & { label: string; tiers: Tier[]; sizes: number[]; paths: string[] }) | null = null;
  let cleanTimer: ReturnType<typeof setInterval> | null = null;
  const batches: Batch[] = [
    {
      id: "20261015T091200Z-0a1b00",
      createdAt: 1_792_055_520,
      label: "Cleaner",
      items: Array.from({ length: 6 }, (_, i): BinItem => ({
        id: i,
        original: `C:\\Users\\test-user\\AppData\\Local\\Spotify\\Data\\chunk-${i + 1}.file`,
        stored: `C:\\Users\\test-user\\AppData\\Local\\WinFastCleaner\\recovery\\bin\\20261015T091200Z-0a1b00\\${i}`,
        size: (i + 1) * 37 * MB,
        tier: "yellow",
        rule: "spotify.cache",
        state: "inBin",
      })),
    },
  ];
  let settingsView: SettingsView = {
    settings: {
      exclusions: [{ kind: "folder", value: "C:\\Users\\test-user\\Projects\\keep" }],
      greenToBin: false,
      bin: { maxBytes: 5 * GB, maxPercent: 10, maxBatches: 5, maxAgeDays: 14 },
      snapshotDays: 90,
    },
    dataDir: "C:\\Users\\test-user\\AppData\\Local\\WinFastCleaner",
    rulesDir: "C:\\Users\\test-user\\AppData\\Local\\WinFastCleaner\\rules",
    userRules: 0,
    ruleErrors: [],
  };
  const summaries = () =>
    batches
      .map((b) => ({
        id: b.id,
        createdAt: b.createdAt,
        label: b.label,
        items: b.items.length,
        size: b.items.reduce((s, i) => s + i.size, 0),
      }))
      .sort((a, b) => b.createdAt - a.createdAt);
  const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));
  let findId = 5000;
  let found: FoundFile[] = [];
  let dupeGroups: DupeGroupView[] = [];
  const manualPlan = (picked: { path: string; size: number }[], label: string) => {
    const size = picked.reduce((s, f) => s + f.size, 0);
    plan = {
      planId: ++findId,
      count: picked.length,
      totalSize: size,
      deleteCount: 0,
      deleteBytes: 0,
      binCount: picked.length,
      binBytes: size,
      notReady: [],
      label,
      tiers: picked.map(() => "red"),
      sizes: picked.map((f) => f.size),
      paths: picked.map((f) => f.path),
    };
    return { plan, sample: [...picked].sort((a, b) => b.size - a.size).slice(0, 100), rejected: [] };
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
    pickFile: async () => "D:\\Videos\\keep-this.mkv",

    startJunkScan: async (onEvent) => {
      await delay(900);
      junkId += 1;
      onEvent({
        type: "done",
        result: {
          scanId: junkId,
          rules,
          errors: [],
          scannedAt: Math.floor(Date.now() / 1000),
          durationMs: 850,
          elevated: false,
        },
      });
    },
    cancelJunkScan: async () => {},
    junkItems: async (_scan, ruleId, offset, limit) => {
      const all = items.get(ruleId) ?? [];
      return { total: all.length, items: all.slice(offset, offset + limit) };
    },
    cleanPreview: async (_scan, selection) => {
      const p: NonNullable<typeof plan> = {
        planId: junkId * 10 + 1,
        count: 0,
        totalSize: 0,
        deleteCount: 0,
        deleteBytes: 0,
        binCount: 0,
        binBytes: 0,
        notReady: [],
        label: "Cleaner",
        tiers: [],
        sizes: [],
        paths: [],
      };
      for (const sel of selection) {
        const r = rules.find((x) => x.id === sel.ruleId);
        if (!r) throw new Error("Unknown rule.");
        if (r.status.kind !== "ready") {
          if (r.status.kind !== "nothingFound") p.notReady.push({ ruleId: r.id, status: r.status });
          continue;
        }
        const off = new Set(sel.deselected);
        for (const item of items.get(r.id) ?? []) {
          if (off.has(item.index)) continue;
          p.count += 1;
          p.totalSize += item.size;
          p.tiers.push(r.tier);
          p.sizes.push(item.size);
          p.paths.push(item.path);
          if (r.tier === "green" && !settingsView.settings.greenToBin) {
            p.deleteCount += 1;
            p.deleteBytes += item.size;
          } else {
            p.binCount += 1;
            p.binBytes += item.size;
          }
        }
      }
      plan = p;
      return p;
    },
    cleanManual: async (scan, nodeIds) => {
      const picked = nodeIds.map(get);
      const rejected = picked.filter((n) => n.protected).map((n) => ({ path: pathOf(n.id), reason: "Protected: Windows folder" }));
      const ok = picked.filter((n) => !n.protected);
      const size = ok.reduce((s, n) => s + n.size, 0);
      const count = ok.reduce((s, n) => s + (n.isDir ? n.files : 1), 0);
      plan = {
        planId: scan * 100 + nodeIds.length,
        count,
        totalSize: size,
        deleteCount: 0,
        deleteBytes: 0,
        binCount: count,
        binBytes: size,
        notReady: [],
        label: "Disk view",
        tiers: ok.map(() => "red"),
        sizes: ok.map((n) => n.size),
        paths: ok.map((n) => pathOf(n.id)),
      };
      return {
        plan,
        sample: ok.slice(0, 100).map((n) => ({ path: pathOf(n.id), size: n.size })),
        rejected,
      };
    },
    cleanExecute: async (planId, onEvent) => {
      if (!plan || plan.planId !== planId) throw new Error("This preview is out of date. Please review it again.");
      const p = plan;
      plan = null;
      let done = 0;
      cleanTimer = setInterval(() => {
        done = Math.min(p.count, done + Math.ceil(p.count / 8));
        onEvent({ type: "progress", done, total: p.count });
        if (done < p.count) return;
        clearInterval(cleanTimer!);
        cleanTimer = null;
        const locked = Math.min(3, p.count);
        const report: CleanReport = {
          deletedCount: p.deleteCount,
          deletedBytes: p.deleteBytes,
          binnedCount: p.binCount,
          binnedBytes: p.binBytes,
          skippedCount: locked,
          skipped: Array.from({ length: locked }, (_, i) => ({
            path: p.paths[i] ?? "C:\\Users\\test-user\\AppData\\Local\\Temp\\locked.tmp",
            reason: "In use by another program",
          })),
          freedBytes: p.deleteBytes + p.binBytes,
          batchId: p.binCount > 0 ? `20261017T1200${String(batches.length).padStart(2, "0")}Z-mock00` : null,
          evicted: { batches: 0, files: 0, bytes: 0, failed: 0 },
          foldersRemoved: 4,
        };
        if (report.batchId) {
          batches.push({
            id: report.batchId,
            createdAt: Math.floor(Date.now() / 1000),
            label: p.label,
            items: p.paths
              .map((path, i) => ({ path, i }))
              .filter(({ i }) => p.tiers[i] !== "green" || settingsView.settings.greenToBin)
              .slice(0, 200)
              .map(({ path, i }) => ({
                id: i,
                original: path,
                stored: `bin\\${i}`,
                size: p.sizes[i] ?? 0,
                tier: p.tiers[i] ?? "red",
                rule: null,
                state: "inBin" as const,
              })),
          });
        }
        // The cleaned files are gone from the next scan.
        rules = rules.map((r) =>
          r.status.kind === "ready" && p.paths.some((x) => (items.get(r.id) ?? []).some((i) => i.path === x))
            ? { ...r, files: 0, size: 0, status: { kind: "nothingFound" } }
            : r,
        );
        onEvent({ type: "done", report });
      }, 200);
    },
    cancelClean: async () => {},
    deepClean: async () => {
      throw new Error("Deep clean needs administrator rights. Restart as administrator first.");
    },

    recoveryOverview: async () => ({
      batches: summaries(),
      usage: [{ drive: "C:\\", bytes: summaries().reduce((s, b) => s + b.size, 0), limit: 5 * GB }],
      limits: settingsView.settings.bin,
    }),
    recoveryBatch: async (id) => {
      const b = batches.find((x) => x.id === id);
      if (!b) throw new Error("Unknown recovery batch.");
      return b;
    },
    recoveryConflicts: async (id, itemIds) => {
      const b = batches.find((x) => x.id === id);
      return (b?.items ?? []).filter((i) => (itemIds ? itemIds.includes(i.id) : true) && i.id === 0).map((i) => i.original);
    },
    recoveryRestore: async (id, itemIds, conflict) => {
      const b = batches.find((x) => x.id === id);
      if (!b) throw new Error("Unknown recovery batch.");
      const chosen = b.items.filter((i) => (itemIds ? itemIds.includes(i.id) : true));
      const blocked = conflict === "skip" ? chosen.filter((i) => i.id === 0) : [];
      const restored = chosen.filter((i) => !blocked.includes(i));
      b.items = b.items.filter((i) => !restored.includes(i));
      if (b.items.length === 0) batches.splice(batches.indexOf(b), 1);
      return {
        restored: restored.length,
        restoredBytes: restored.reduce((s, i) => s + i.size, 0),
        renamed: conflict === "keepBoth" ? restored.filter((i) => i.id === 0).map((i) => i.original + " (restored)") : [],
        skipped: blocked.map((i) => [i.original, "A file with that name already exists"] as [string, string]),
      };
    },
    recoveryDelete: async (id) => {
      const i = batches.findIndex((x) => x.id === id);
      if (i < 0) return { batches: 0, files: 0, bytes: 0, failed: 0 };
      const [b] = batches.splice(i, 1);
      return { batches: 1, files: b!.items.length, bytes: b!.items.reduce((s, x) => s + x.size, 0), failed: 0 };
    },
    recoveryEmpty: async () => {
      const files = batches.reduce((s, b) => s + b.items.length, 0);
      const bytes = batches.reduce((s, b) => s + b.items.reduce((t, x) => t + x.size, 0), 0);
      const n = batches.length;
      batches.length = 0;
      return { batches: n, files, bytes, failed: 0 };
    },

    getSettings: async () => settingsView,
    saveSettings: async (settings) => {
      for (const e of settings.exclusions) {
        if (e.kind !== "pattern" && !/^[A-Za-z]:\\/.test(e.value)) throw new Error(`\`${e.value}\` is not a usable path.`);
      }
      settingsView = { ...settingsView, settings: structuredClone(settings) };
      return settingsView;
    },
    excludeNode: async (_scan, nodeId) => {
      const n = get(nodeId);
      n.excluded = true;
      settingsView.settings.exclusions.push({ kind: n.isDir ? "folder" : "file", value: pathOf(nodeId) });
    },
    openRulesFolder: async () => {},

    startFind: async (path, request, _rescan, onEvent) => {
      for (let i = 1; i <= 5; i++) {
        await delay(150);
        onEvent({ type: "scanning", files: i * 41_000, bytes: i * 21 * GB });
      }
      let files = mockFound(path, request.mode === "old").filter((f) => f.size >= request.minSize);
      if (request.kinds.length > 0) files = files.filter((f) => request.kinds.includes(f.kind));
      found = files;
      onEvent({
        type: "files",
        result: {
          findId: ++findId,
          root: path,
          reusedScan: path === "C:\\",
          scannedAt: Math.floor(Date.now() / 1000),
          files,
          more: 0,
          hidden: 4,
          recentlyOpened: request.mode === "old" ? 2 : 0,
        },
      });
    },
    startDupes: async (path, _minSize, _kinds, _rescan, onEvent) => {
      onEvent({ type: "comparing", phase: "sizes", done: 0, total: 0 });
      for (let i = 1; i <= 6; i++) {
        await delay(150);
        onEvent({ type: "comparing", phase: "hashing", done: i * 700 * MB, total: 4.2 * GB });
      }
      dupeGroups = mockDupes(path);
      onEvent({
        type: "dupes",
        result: {
          dupesId: ++findId,
          root: path,
          reusedScan: false,
          scannedAt: Math.floor(Date.now() / 1000),
          groups: dupeGroups,
          wasted: dupeGroups.reduce((s, g) => s + g.wasted, 0),
          filesHashed: 7,
          bytesHashed: 4.2 * GB,
          unreadable: 1,
          hidden: 3,
        },
      });
    },
    cancelFind: async () => {},
    filesPlan: async (_id, indexes) => {
      const picked = indexes.map((i) => found[i]!).filter(Boolean);
      return manualPlan(picked.map((f) => ({ path: f.path, size: f.size })), "Large and old files");
    },
    dupesPlan: async (_id, picks) => {
      const byGroup = new Map<number, number>();
      for (const [g] of picks) byGroup.set(g, (byGroup.get(g) ?? 0) + 1);
      for (const [g, n] of byGroup) {
        if (n >= (dupeGroups[g]?.files.length ?? 0)) throw new Error("Keep at least one copy. Every copy was selected.");
      }
      return manualPlan(
        picks.map(([g, f]) => ({ path: dupeGroups[g]!.files[f]!.path, size: dupeGroups[g]!.size })),
        "Duplicates",
      );
    },

    listSnapshots: async () => [
      { key: "C_-1-20261017T090000Z.snapshot.json", root: "C:\\", takenAt: 1_792_227_600, size: 106 * GB },
      { key: "C_-1-20261009T090000Z.snapshot.json", root: "C:\\", takenAt: 1_791_536_400, size: 101 * GB },
      { key: "C_-1-20260920T090000Z.snapshot.json", root: "C:\\", takenAt: 1_789_894_800, size: 98 * GB },
      { key: "D_-2-20261012T090000Z.snapshot.json", root: "D:\\", takenAt: 1_791_795_600, size: 640 * GB },
    ],
    compareSnapshots: async (older, newer): Promise<ComparisonView> => {
      const days = older.includes("20260920") ? 2 : 1;
      const u = "C:\\Users\\test-user\\";
      return {
        root: "C:\\",
        oldTakenAt: older.includes("20260920") ? 1_789_894_800 : 1_791_536_400,
        newTakenAt: newer.includes("20261017") ? 1_792_227_600 : 1_791_536_400,
        totalDelta: days * 4.6 * GB,
        threshold: 50 * MB,
        grew: [
          { path: `${u}AppData\\Roaming\\discord\\Cache`, oldSize: 120 * MB, newSize: 120 * MB + days * 3.1 * GB, delta: days * 3.1 * GB, rule: { id: "discord.cache", name: "Discord cache" } },
          { path: `${u}Videos\\Captures`, oldSize: null, newSize: days * 900 * MB, delta: days * 900 * MB, rule: null },
          { path: `${u}AppData\\Local\\npm-cache`, oldSize: 400 * MB, newSize: 400 * MB + days * 610 * MB, delta: days * 610 * MB, rule: { id: "dev.npm-cache", name: "npm cache" } },
          { path: "C:\\Windows\\SoftwareDistribution\\Download", oldSize: 300 * MB, newSize: 300 * MB + 240 * MB, delta: 240 * MB, rule: { id: "windows.update-downloads", name: "Windows Update downloads" } },
        ],
        shrank: [{ path: `${u}Downloads`, oldSize: 6 * GB, newSize: 5 * GB, delta: -1 * GB, rule: null }],
      };
    },
    locateNode: async (_scan, path) => {
      const id = nodes.findIndex((_, i) => pathOf(i) === path);
      return id >= 0 ? id : null;
    },
  };
}
