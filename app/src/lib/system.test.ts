import assert from "node:assert/strict";
import { test } from "node:test";

import {
  canToggle,
  driveStatus,
  latestPair,
  matchesFilter,
  sortChecks,
  sortStartup,
  STATUS,
  topGrowth,
  updatable,
} from "./system.ts";
import type { Check, DriveInfo, HealthThresholds, SnapshotView, StartupItem } from "./types.ts";

const GB = 1024 ** 3;
const T: HealthThresholds = {
  diskFreeYellowPct: 15,
  diskFreeRedPct: 5,
  junkYellowGb: 5,
  junkRedGb: 20,
  startupYellow: 10,
  startupRed: 20,
  binYellowPct: 90,
  ssdWearYellowPct: 80,
};

function check(id: Check["id"], status: Check["status"]): Check {
  return { id, title: id, status, summary: "", details: [], fix: null, asOf: null };
}

function drive(free: number, kind: DriveInfo["kind"] = "fixed"): DriveInfo {
  return { root: "C:\\", label: "", fileSystem: "NTFS", kind, total: 100 * GB, free: free * GB, isSystem: true };
}

function item(name: string, enabled: boolean, machine = false): StartupItem {
  return {
    id: `user-run:${name}`,
    name,
    source: machine ? "machineRun" : "userRun",
    location: "",
    command: `C:\\Apps\\${name}.exe`,
    program: null,
    publisher: name === "b" ? "Vendor Inc." : null,
    enabled,
    machine,
    disabledAt: null,
  };
}

test("every status has a label and an icon", () => {
  for (const s of Object.values(STATUS)) {
    assert.ok(s.label.length > 0);
    assert.ok(s.icon.length > 0);
  }
});

test("checks needing attention come first", () => {
  const sorted = sortChecks([check("diskSpace", "green"), check("junk", "yellow"), check("temperatures", "unknown"), check("startup", "red")]);
  assert.deepEqual(
    sorted.map((c) => c.id),
    ["startup", "junk", "diskSpace", "temperatures"],
  );
});

test("drive status matches the health thresholds", () => {
  assert.equal(driveStatus(drive(20), T), "green");
  assert.equal(driveStatus(drive(15), T), "green");
  assert.equal(driveStatus(drive(14), T), "yellow");
  assert.equal(driveStatus(drive(4), T), "red");
  assert.equal(driveStatus(drive(4, "removable"), T), "unknown");
});

test("the latest two snapshots of a drive are compared", () => {
  const s = (key: string, root: string, takenAt: number): SnapshotView => ({ key, root, takenAt, size: 0 });
  const list = [s("a", "C:\\", 1), s("b", "D:\\", 9), s("c", "c:\\", 5), s("d", "C:\\", 3)];
  assert.deepEqual(latestPair(list, "C:\\")?.map((x) => x.key), ["c", "d"]);
  assert.equal(latestPair([s("a", "C:\\", 1)], "C:\\"), null);
});

test("top growth ignores shrinking folders", () => {
  const c = (path: string, delta: number) => ({ path, oldSize: 0, newSize: delta, delta, rule: null });
  assert.deepEqual(
    topGrowth([c("a", 5), c("b", -9), c("c", 10), c("d", 1), c("e", 7)], 3).map((x) => x.path),
    ["c", "e", "a"],
  );
});

test("startup items sort, filter, and respect admin rights", () => {
  assert.deepEqual(
    sortStartup([item("b", false), item("C", true), item("a", true)]).map((i) => i.name),
    ["a", "C", "b"],
  );
  assert.ok(matchesFilter(item("b", true), "vendor"));
  assert.ok(matchesFilter(item("b", true), "registry"));
  assert.ok(!matchesFilter(item("a", true), "vendor"));
  assert.ok(canToggle(item("a", true), false));
  assert.ok(!canToggle(item("a", true, true), false));
  assert.ok(canToggle(item("a", true, true), true));
});

test("apps winget can't take back are not offered", () => {
  const u = (id: string, canUpdate: boolean) => ({ name: id, id, version: "1", available: "2", source: "winget", canUpdate });
  assert.deepEqual(
    updatable([u("A.A", true), u("B.B…", false)]).map((x) => x.id),
    ["A.A"],
  );
});
