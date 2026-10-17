// Pure helpers for the Home, Startup, and Updates tabs (kept out of
// components so they can be unit tested).

import type {
  ChangeView,
  Check,
  DriveInfo,
  Fix,
  HealthStatus,
  HealthThresholds,
  SnapshotView,
  StartupItem,
  StartupSource,
  Upgrade,
} from "./types.ts";

/** Every status is shown with an icon and a word, never color alone. */
export const STATUS: Record<HealthStatus, { label: string; icon: "check" | "alert" | "help" }> = {
  green: { label: "Good", icon: "check" },
  yellow: { label: "Attention", icon: "alert" },
  red: { label: "Problem", icon: "alert" },
  unknown: { label: "Unknown", icon: "help" },
};

export const FIX_LABEL: Record<Fix, string> = {
  disk: "Open Disk",
  cleaner: "Open Cleaner",
  startup: "Open Startup",
  updates: "Open Updates",
  recover: "Open Recover",
};

/** Checks that need attention first, then the rest in their usual order. */
export function sortChecks(checks: Check[]): Check[] {
  const rank: Record<HealthStatus, number> = { red: 0, yellow: 1, green: 2, unknown: 3 };
  return checks
    .map((c, i) => ({ c, i }))
    .sort((a, b) => rank[a.c.status] - rank[b.c.status] || a.i - b.i)
    .map((x) => x.c);
}

/** A drive's free-space status, with the same thresholds as the health check. */
export function driveStatus(d: DriveInfo, t: HealthThresholds): HealthStatus {
  if (d.kind !== "fixed" || d.total <= 0) return "unknown";
  const pct = (d.free * 100) / d.total;
  if (pct < t.diskFreeRedPct) return "red";
  if (pct < t.diskFreeYellowPct) return "yellow";
  return "green";
}

/** The newest two snapshots of `root`, newest first, if there are two. */
export function latestPair(snapshots: SnapshotView[], root: string): [SnapshotView, SnapshotView] | null {
  const list = snapshots.filter((s) => s.root.toLowerCase() === root.toLowerCase()).sort((a, b) => b.takenAt - a.takenAt);
  return list.length >= 2 ? [list[0]!, list[1]!] : null;
}

/** The biggest growth first. */
export function topGrowth(grew: ChangeView[], n: number): ChangeView[] {
  return [...grew]
    .filter((c) => c.delta > 0)
    .sort((a, b) => b.delta - a.delta)
    .slice(0, n);
}

export const SOURCE_LABEL: Record<StartupSource, string> = {
  userRun: "Registry (you)",
  machineRun: "Registry (all users)",
  machineRun32: "Registry (all users, 32-bit)",
  userFolder: "Startup folder (you)",
  commonFolder: "Startup folder (all users)",
  task: "Scheduled task",
};

/** Items in a stable order: on first, then by name. */
export function sortStartup(items: StartupItem[]): StartupItem[] {
  return [...items].sort(
    (a, b) => Number(b.enabled) - Number(a.enabled) || a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
  );
}

/** Whether this item can be switched right now. */
export function canToggle(item: StartupItem, elevated: boolean): boolean {
  return !item.machine || elevated;
}

export function matchesFilter(item: StartupItem, filter: string): boolean {
  const f = filter.trim().toLowerCase();
  if (!f) return true;
  return [item.name, item.publisher ?? "", item.command, SOURCE_LABEL[item.source]].some((s) => s.toLowerCase().includes(f));
}

/** Apps that can be picked for updating. */
export function updatable(list: Upgrade[]): Upgrade[] {
  return list.filter((u) => u.canUpdate);
}
