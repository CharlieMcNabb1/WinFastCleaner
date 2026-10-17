// Pure helpers for the Cleaner and Recover tabs (kept out of components so
// they can be unit tested).

import { formatCount, formatSize } from "./format.ts";
import type { RuleStatus, RuleView, Tier } from "./types.ts";

/** Tier labels and explanations. The colors live in app.css; a tier is
 *  always shown with its label and icon too, never by color alone. */
export const TIERS: Record<Tier, { label: string; icon: "check" | "info" | "alert"; meaning: string }> = {
  green: {
    label: "Safe",
    icon: "check",
    meaning: "Regenerates automatically. Selected by default and deleted directly.",
  },
  yellow: {
    label: "Optional",
    icon: "info",
    meaning: "Safe, but has a cost (for example a re-download). Never selected by default; goes to the recovery bin.",
  },
  red: {
    label: "Review",
    icon: "alert",
    meaning: "Your own files. Never selected automatically; always goes to the recovery bin.",
  },
};

export function isReady(rule: RuleView): boolean {
  return rule.status.kind === "ready";
}

/** Green rules that are ready start ticked; nothing else ever does. */
export function selectedByDefault(rule: RuleView): boolean {
  return rule.tier === "green" && isReady(rule);
}

export function statusText(status: RuleStatus): string {
  switch (status.kind) {
    case "ready":
      return "";
    case "needsAdmin":
      return "Needs administrator rights";
    case "appRunning":
      return `Close ${status.detail.map(appName).join(", ")} first`;
    case "nothingFound":
      return "Nothing to clean";
  }
}

/** "chrome.exe" -> "chrome" */
export function appName(exe: string): string {
  return exe.replace(/\.exe$/i, "");
}

export interface RuleGroup {
  name: string;
  rules: RuleView[];
  size: number;
  files: number;
}

/** Rules grouped by app or area: Windows first, then by size. */
export function groupRules(rules: RuleView[]): RuleGroup[] {
  const groups = new Map<string, RuleGroup>();
  for (const r of rules) {
    let g = groups.get(r.group);
    if (!g) {
      g = { name: r.group, rules: [], size: 0, files: 0 };
      groups.set(r.group, g);
    }
    g.rules.push(r);
    g.size += r.size;
    g.files += r.files;
  }
  for (const g of groups.values()) {
    g.rules.sort((a, b) => b.size - a.size || a.name.localeCompare(b.name));
  }
  return [...groups.values()].sort((a, b) => {
    if (a.name === "Windows") return -1;
    if (b.name === "Windows") return 1;
    return b.size - a.size || a.name.localeCompare(b.name);
  });
}

/** Files unticked in a preview: rule id -> (file index -> size). */
export type Deselected = Map<string, Map<number, number>>;

export function selectionTotals(
  rules: RuleView[],
  selected: Set<string>,
  deselected: Deselected,
): { size: number; count: number } {
  let size = 0;
  let count = 0;
  for (const r of rules) {
    if (!selected.has(r.id) || !isReady(r)) continue;
    const off = deselected.get(r.id);
    let offSize = 0;
    for (const s of off?.values() ?? []) offSize += s;
    size += r.size - offSize;
    count += r.files - (off?.size ?? 0);
  }
  return { size, count };
}

/** "4.2 GB (1,382 files)" */
export function sizeAndCount(size: number, count: number): string {
  return `${formatSize(size)} (${formatCount(count)} ${count === 1 ? "file" : "files"})`;
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${formatCount(n)} ${n === 1 ? one : many}`;
}
