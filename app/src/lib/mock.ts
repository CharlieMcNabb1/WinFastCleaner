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
  HealthView,
  JunkItem,
  LastRun,
  NodeDetail,
  NodeView,
  PlanView,
  RuleView,
  ScanSummary,
  ScheduleView,
  ServiceItem,
  SettingsView,
  StartupItem,
  Tier,
  TraceInfo,
  TreemapNode,
  UpdatesSummary,
  Upgrade,
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

function mockStartup(): StartupItem[] {
  const u = "C:\\Users\\test-user\\AppData";
  const run = "HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
  const hklm = "HKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
  const folder = `${u}\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup`;
  const item = (
    id: string,
    name: string,
    source: StartupItem["source"],
    location: string,
    program: string,
    publisher: string | null,
    enabled = true,
  ): StartupItem => ({
    id,
    name,
    source,
    location,
    command: program.includes(" ") ? `"${program}" --background` : `${program} --background`,
    program,
    publisher,
    enabled,
    machine: source === "machineRun" || source === "machineRun32" || source === "commonFolder",
    disabledAt: enabled ? null : 1_791_000_000,
  });
  return [
    item("user-run:Discord", "Discord", "userRun", run, `${u}\\Local\\Discord\\Update.exe`, "Discord Inc."),
    item("user-run:Spotify", "Spotify", "userRun", run, `${u}\\Roaming\\Spotify\\Spotify.exe`, "Spotify Ltd", false),
    item("user-run:OneDrive", "OneDrive", "userRun", run, "C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe", "Microsoft Corporation"),
    item("user-run:Steam", "Steam", "userRun", run, "C:\\Program Files (x86)\\Steam\\steam.exe", "Valve Corporation"),
    item("user-run:Teams", "Microsoft Teams", "userRun", run, `${u}\\Local\\Microsoft\\Teams\\Update.exe`, "Microsoft Corporation"),
    item("machine-run:SecurityHealth", "SecurityHealth", "machineRun", hklm, "C:\\Windows\\System32\\SecurityHealthSystray.exe", "Microsoft Corporation"),
    item("machine-run:RtkAudUService", "RtkAudUService", "machineRun", hklm, "C:\\Windows\\System32\\RtkAudUService64.exe", "Realtek Semiconductor"),
    item("machine-run32:Adobe CCXProcess", "Adobe CCXProcess", "machineRun32", hklm, "C:\\Program Files (x86)\\Adobe\\Adobe Creative Cloud Experience\\CCXProcess.exe", "Adobe Inc."),
    item("user-folder:Notes helper.lnk", "Notes helper", "userFolder", folder, "C:\\Tools\\notes-helper.exe", null),
    item("user-folder:sync.cmd", "sync", "userFolder", folder, `${folder}\\sync.cmd`, null, false),
    item("task:\\GoogleUpdateTaskMachineCore", "GoogleUpdateTaskMachineCore", "task", "Task Scheduler \\", "C:\\Program Files (x86)\\Google\\Update\\GoogleUpdate.exe", "Google LLC"),
    item("task:\\Vendor\\Tray at logon", "Tray at logon", "task", "Task Scheduler \\Vendor", "C:\\Program Files\\Vendor\\tray.exe", "Vendor Corp"),
  ];
}

function mockServices(): ServiceItem[] {
  return [
    { name: "AdobeARMservice", displayName: "Adobe Acrobat Update Service", program: "C:\\Program Files (x86)\\Common Files\\Adobe\\ARM\\1.0\\armsvc.exe", publisher: "Adobe Inc.", running: true, delayed: false },
    { name: "edgeupdate", displayName: "Microsoft Edge Update Service (edgeupdate)", program: "C:\\Program Files (x86)\\Microsoft\\EdgeUpdate\\MicrosoftEdgeUpdate.exe", publisher: "Microsoft Corporation", running: false, delayed: true },
    { name: "Steam Client Service", displayName: "Steam Client Service", program: "C:\\Program Files (x86)\\Common Files\\Steam\\steamservice.exe", publisher: "Valve Corporation", running: false, delayed: false },
  ];
}

function mockUpgrades(): Upgrade[] {
  return [
    { name: "Git", id: "Git.Git", version: "2.44.0", available: "2.45.1", source: "winget", canUpdate: true },
    { name: "Mozilla Firefox (x64 en-US)", id: "Mozilla.Firefox", version: "125.0.3", available: "126.0.1", source: "winget", canUpdate: true },
    { name: "Microsoft Visual C++ 2015-2022 Redistributable", id: "Microsoft.VCRedist.2015+.x64", version: "14.38.33135.0", available: "14.40.33810.0", source: "winget", canUpdate: true },
    { name: "7-Zip", id: "7zip.7zip", version: "23.01", available: "24.07", source: "winget", canUpdate: true },
    { name: "A long app name here", id: "Some.Really.Long.Package.I…", version: "1.0", available: "2.0", source: "winget", canUpdate: false },
  ];
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
  // Startup, updates, and health state.
  let startupItems = mockStartup();
  let upgrades: Upgrade[] = mockUpgrades();
  let lastUpdates: UpdatesSummary | null = null;
  let applying = false;
  let cancelApply = false;
  let junkScanned: { at: number; bytes: number; files: number } | null = null;
  let taskExists = false;
  let trayRunning = false;
  const lastRun: LastRun = {
    startedAt: 1_791_550_800,
    finishedAt: 1_791_550_806,
    ok: true,
    message: "Cleaned 1.4 GB (2,118 files). Skipped 1 rule (see the details).",
    deletedCount: 2118,
    deletedBytes: 1.4 * GB,
    binnedCount: 0,
    binnedBytes: 0,
    skippedCount: 3,
    freedBytes: 1.4 * GB,
    batchId: null,
    notReady: [{ rule: "Windows Update downloads", reason: "needs administrator rights" }],
  };
  let settingsView: SettingsView = {
    settings: {
      exclusions: [{ kind: "folder", value: "C:\\Users\\test-user\\Projects\\keep" }],
      greenToBin: false,
      bin: { maxBytes: 5 * GB, maxPercent: 10, maxBatches: 5, maxAgeDays: 14 },
      snapshotDays: 90,
      secureDelete: false,
      health: {
        diskFreeYellowPct: 15,
        diskFreeRedPct: 5,
        junkYellowGb: 5,
        junkRedGb: 20,
        startupYellow: 10,
        startupRed: 20,
        binYellowPct: 90,
        ssdWearYellowPct: 80,
      },
      schedule: {
        enabled: false,
        frequency: "weekly",
        weekday: "sunday",
        hour: 12,
        minute: 0,
        includeOptional: false,
        rules: [],
      },
      tray: false,
      theme: "system",
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
  const traces: TraceInfo[] = [
    { trace: "recentDocuments", name: "Recent documents", description: "The list of files you opened recently, shown in File Explorer's Quick access and Start.", lose: "The recent files lists. The files themselves are not touched.", items: 148, available: true },
    { trace: "jumpLists", name: "Jump lists", description: "The recent and frequent items shown when you right-click an app on the taskbar or Start.", lose: "Recent items in jump lists, and items you pinned to them. The files themselves are not touched.", items: 37, available: true },
    { trace: "runHistory", name: "Run history", description: "Commands you typed in the Run dialog (Windows key + R).", lose: "The Run dialog's suggestions.", items: 6, available: true },
    { trace: "clipboard", name: "Clipboard", description: "What is copied right now.", lose: "What you copied. Clipboard history (Windows key + V) is kept; clear it in Windows Settings.", items: 1, available: true },
  ];
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
    appInfo: async () => ({
      name: "WinFastCleaner",
      version: "0.1.0 (mock)",
      // `?platform=linux` previews the Linux layout.
      platform: new URLSearchParams(location.search).get("platform") ?? "windows",
      elevated: false,
    }),
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
      junkScanned = {
        at: Math.floor(Date.now() / 1000),
        bytes: rules.reduce((s, r) => s + r.size, 0),
        files: rules.reduce((s, r) => s + r.files, 0),
      };
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
    cleanExecute: async (planId, secure, onEvent) => {
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
        const wiped = secure ? p.tiers.filter((t) => t !== "red").length : 0;
        const report: CleanReport = {
          deletedCount: p.deleteCount + (secure ? p.binCount - p.tiers.filter((t) => t === "red").length : 0),
          deletedBytes: p.deleteBytes,
          wipedCount: wiped,
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
    recoveryDelete: async (id, _secure) => {
      const i = batches.findIndex((x) => x.id === id);
      if (i < 0) return { batches: 0, files: 0, bytes: 0, failed: 0 };
      const [b] = batches.splice(i, 1);
      return { batches: 1, files: b!.items.length, bytes: b!.items.reduce((s, x) => s + x.size, 0), failed: 0 };
    },
    recoveryEmpty: async (_secure) => {
      const files = batches.reduce((s, b) => s + b.items.length, 0);
      const bytes = batches.reduce((s, b) => s + b.items.reduce((t, x) => t + x.size, 0), 0);
      const n = batches.length;
      batches.length = 0;
      return { batches: n, files, bytes, failed: 0 };
    },

    getSettings: async () => settingsView,
    saveSettings: async (incoming) => {
      // Like the real backend: the schedule and the tray only change
      // through their own commands.
      const settings = {
        ...incoming,
        schedule: settingsView.settings.schedule,
        tray: settingsView.settings.tray,
      };
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
    privacySurvey: async () => traces.map((t) => ({ ...t })),
    privacyClear: async (chosen) => {
      await delay(400);
      return chosen.map((trace) => {
        const t = traces.find((x) => x.trace === trace)!;
        const cleared = t.items ?? 0;
        t.items = 0;
        return trace === "jumpLists"
          ? {
              trace,
              cleared: cleared - 1,
              skipped: [{ path: "C:\\Users\\test-user\\AppData\\Roaming\\Microsoft\\Windows\\Recent\\AutomaticDestinations\\f01b4d95cf55d32a.automaticDestinations-ms", reason: "In use by another program" }],
              error: null,
            }
          : { trace, cleared, skipped: [], error: null };
      });
    },
    locateNode: async (_scan, path) => {
      const id = nodes.findIndex((_, i) => pathOf(i) === path);
      return id >= 0 ? id : null;
    },
    startupList: async () => {
      await delay(250);
      return {
        items: startupItems.map((i) => ({ ...i })),
        errors: [],
        services: mockServices(),
        servicesError: null,
        elevated: false,
      };
    },
    startupSetEnabled: async (id, enabled) => {
      await delay(200);
      const item = startupItems.find((i) => i.id === id);
      if (!item) throw new Error("This item is no longer in the startup list. Refresh and try again.");
      if (item.machine) {
        throw new Error(
          "Windows needs administrator rights to change an item that starts for every user. Restart as administrator and try again.",
        );
      }
      item.enabled = enabled;
      item.disabledAt = enabled ? null : Math.floor(Date.now() / 1000);
      return { ...item };
    },

    updatesStatus: async () => ({ installed: true, missingMessage: null, last: lastUpdates }),
    updatesList: async () => {
      await delay(1500);
      lastUpdates = { checkedAt: Math.floor(Date.now() / 1000), count: upgrades.length };
      return upgrades.map((u) => ({ ...u }));
    },
    updatesApply: async (ids, onEvent) => {
      if (applying) throw new Error("winget is already running.");
      applying = true;
      cancelApply = false;
      void (async () => {
        let cancelled = false;
        for (const [index, id] of ids.entries()) {
          if (cancelApply) {
            cancelled = true;
            break;
          }
          onEvent({ type: "started", id, index, total: ids.length });
          await delay(1200);
          const up = upgrades.find((u) => u.id === id);
          const ok = id !== "7zip.7zip";
          onEvent({
            type: "finished",
            result: {
              id,
              ok,
              message: ok
                ? `Updated to ${up?.available ?? "the latest version"}.`
                : "The update failed (code 0x8A15010B): The installer failed with exit code 1603.",
            },
          });
          if (ok) upgrades = upgrades.filter((u) => u.id !== id);
        }
        applying = false;
        onEvent({ type: "done", cancelled });
      })();
    },
    updatesCancel: async () => {
      cancelApply = true;
    },

    healthCheck: async (): Promise<HealthView> => {
      await delay(300);
      const t = settingsView.settings.health;
      const enabled = startupItems.filter((i) => i.enabled).length;
      const c = drives.find((d) => d.isSystem)!;
      const pctFree = Math.floor((c.free / c.total) * 100);
      const junkBytes = junkScanned?.bytes ?? null;
      const checks: HealthView["checks"] = [
        {
          id: "diskSpace",
          title: "Disk space",
          status: pctFree < t.diskFreeRedPct ? "red" : pctFree < t.diskFreeYellowPct ? "yellow" : "green",
          summary:
            pctFree < t.diskFreeYellowPct
              ? `C: has ${pctFree}% free (61.0 GB of 476.0 GB).`
              : `Every drive has at least ${t.diskFreeYellowPct}% free.`,
          details: ["C: has 12% free (61.0 GB of 476.0 GB).", "D: has 64% free (1.2 TB of 1.8 TB)."],
          fix: pctFree < t.diskFreeYellowPct ? "cleaner" : null,
          asOf: null,
        },
        junkBytes === null
          ? { id: "junk", title: "Junk", status: "unknown", summary: "Not scanned yet.", details: [], fix: "cleaner", asOf: null }
          : {
              id: "junk",
              title: "Junk",
              status: junkBytes > t.junkRedGb * GB ? "red" : junkBytes > t.junkYellowGb * GB ? "yellow" : "green",
              summary: `${(junkBytes / GB).toFixed(1)} GB can be cleaned (${junkScanned!.files} files).`,
              details: [],
              fix: junkBytes > t.junkYellowGb * GB ? "cleaner" : null,
              asOf: junkScanned!.at,
            },
        {
          id: "startup",
          title: "Startup apps",
          status: enabled > t.startupRed ? "red" : enabled > t.startupYellow ? "yellow" : "green",
          summary: `${enabled} apps start when you sign in.`,
          details: [],
          fix: enabled > t.startupYellow ? "startup" : null,
          asOf: null,
        },
        {
          id: "driveHealth",
          title: "Drive health",
          status: "green",
          summary: "All 2 drives report they are healthy.",
          details: ["Samsung SSD 980 PRO 1TB (SSD): healthy, 4% of rated life used", "WDC WD20EZAZ (HDD): healthy"],
          fix: null,
          asOf: null,
        },
        {
          id: "temperatures",
          title: "Temperatures",
          status: "unknown",
          summary: "Not available yet.",
          details: ["Reading CPU and GPU temperatures needs a hardware sensor driver, which WinFastCleaner doesn't install."],
          fix: null,
          asOf: null,
        },
        lastUpdates === null
          ? { id: "updates", title: "App updates", status: "unknown", summary: "Not checked yet.", details: [], fix: "updates", asOf: null }
          : {
              id: "updates",
              title: "App updates",
              status: lastUpdates.count > 0 ? "yellow" : "green",
              summary: lastUpdates.count > 0 ? `${lastUpdates.count} app updates available.` : "Your apps are up to date.",
              details: [],
              fix: lastUpdates.count > 0 ? "updates" : null,
              asOf: lastUpdates.checkedAt,
            },
        {
          id: "recoveryBin",
          title: "Recovery bin",
          status: "green",
          summary: `${(batches.reduce((s, b) => s + b.items.reduce((a, i) => a + i.size, 0), 0) / MB).toFixed(1)} MB in the recovery bin.`,
          details: [],
          fix: "recover",
          asOf: null,
        },
      ];
      const order = ["unknown", "green", "yellow", "red"] as const;
      const overall = checks.reduce((w, c) => (order.indexOf(c.status) > order.indexOf(w) ? c.status : w), "unknown" as HealthView["overall"]);
      return { checks, overall };
    },
    scheduleStatus: async (): Promise<ScheduleView> => ({
      schedule: structuredClone(settingsView.settings.schedule),
      taskExists: settingsView.settings.schedule.enabled ? taskExists : false,
      programFound: true,
      last: lastRun,
      rules: rules.filter((r) => r.tier !== "red").map((r) => ({ id: r.id, name: r.name, group: r.group, tier: r.tier })),
    }),
    scheduleSave: async (schedule) => {
      await delay(300);
      if (schedule.hour > 23 || schedule.minute > 59) throw new Error("scheduled cleaning: the time of day is not valid");
      taskExists = schedule.enabled;
      settingsView = { ...settingsView, settings: { ...settingsView.settings, schedule: structuredClone(schedule) } };
      return {
        schedule: structuredClone(schedule),
        taskExists,
        programFound: true,
        last: lastRun,
        rules: rules.filter((r) => r.tier !== "red").map((r) => ({ id: r.id, name: r.name, group: r.group, tier: r.tier })),
      };
    },
    trayStatus: async () => ({ enabled: settingsView.settings.tray, running: trayRunning, programFound: true }),
    traySet: async (enabled) => {
      await delay(200);
      trayRunning = enabled;
      settingsView = { ...settingsView, settings: { ...settingsView.settings, tray: enabled } };
      return { enabled, running: trayRunning, programFound: true };
    },
  };
}
