import assert from "node:assert/strict";
import { test } from "node:test";

import { groupRules, selectedByDefault, selectionTotals, sizeAndCount, statusText } from "./cleaner.ts";
import type { RuleView } from "./types.ts";

function rule(id: string, group: string, tier: RuleView["tier"], size: number, files: number, ready = true): RuleView {
  return {
    id,
    name: id,
    group,
    tier,
    description: "",
    lose: "",
    requiresAdmin: false,
    closeApps: [],
    minAgeDays: null,
    userRule: false,
    status: ready ? { kind: "ready" } : { kind: "needsAdmin" },
    files,
    size,
    hidden: 0,
  };
}

test("only ready green rules are selected by default", () => {
  assert.equal(selectedByDefault(rule("a", "W", "green", 1, 1)), true);
  assert.equal(selectedByDefault(rule("b", "W", "yellow", 1, 1)), false);
  assert.equal(selectedByDefault(rule("c", "W", "red", 1, 1)), false);
  assert.equal(selectedByDefault(rule("d", "W", "green", 1, 1, false)), false);
});

test("groups put Windows first, then the largest", () => {
  const groups = groupRules([
    rule("a", "Apps", "green", 10, 1),
    rule("b", "Windows", "green", 1, 1),
    rule("c", "Chrome", "green", 50, 1),
    rule("d", "Apps", "yellow", 30, 1),
  ]);
  assert.deepEqual(
    groups.map((g) => g.name),
    ["Windows", "Chrome", "Apps"],
  );
  assert.equal(groups[2]!.size, 40);
  assert.deepEqual(
    groups[2]!.rules.map((r) => r.id),
    ["d", "a"],
  );
});

test("totals skip unselected, not-ready, and unticked files", () => {
  const rules = [
    rule("a", "W", "green", 1000, 10),
    rule("b", "W", "yellow", 500, 5),
    rule("c", "W", "green", 700, 7, false),
  ];
  const deselected = new Map([["a", new Map([[3, 100]])]]);
  assert.deepEqual(selectionTotals(rules, new Set(["a", "c"]), deselected), { size: 900, count: 9 });
  assert.deepEqual(selectionTotals(rules, new Set(["a", "b"]), new Map()), { size: 1500, count: 15 });
});

test("labels", () => {
  assert.equal(sizeAndCount(4_509_715_660, 1382), `4.2 GB (${(1382).toLocaleString()} files)`);
  assert.equal(sizeAndCount(10, 1), "10 B (1 file)");
  assert.equal(statusText({ kind: "appRunning", detail: ["chrome.exe", "msedge.exe"] }), "Close chrome, msedge first");
});
