import assert from "node:assert/strict";
import { test } from "node:test";

import { canPick, type DupePicks, pickAllButOne, pickTotals, signedSize, splitPath } from "./files.ts";
import type { DupeGroupView } from "./types.ts";

function group(index: number, size: number, modified: number[]): DupeGroupView {
  return {
    index,
    size,
    hash: "h",
    wasted: size * (modified.length - 1),
    files: modified.map((m, i) => ({ index: i, path: `C:\\d${i}\\f.bin`, modified: m })),
  };
}

test("paths split into name and folder", () => {
  assert.deepEqual(splitPath("C:\\Users\\a\\b.txt"), { name: "b.txt", folder: "C:\\Users\\a" });
  assert.deepEqual(splitPath("/tmp/x"), { name: "x", folder: "/tmp" });
  assert.deepEqual(splitPath("x"), { name: "x", folder: "" });
});

test("the last copy in a group can never be picked", () => {
  const g = group(0, 100, [1, 2, 3]);
  const picks: DupePicks = new Map([[0, new Set([0, 1])]]);
  assert.equal(canPick(g, picks, 2), false);
  assert.equal(canPick(g, picks, 1), true, "already picked can be unpicked");
  assert.equal(canPick(g, new Map(), 0), true);
});

test("pick all but one keeps the newest or the oldest", () => {
  const groups = [group(0, 100, [10, 30, 20]), group(1, 50, [5, 1])];
  const newest = pickAllButOne(groups, "newest");
  assert.deepEqual([...newest.get(0)!].sort(), [0, 2]);
  assert.deepEqual([...newest.get(1)!], [1]);
  const oldest = pickAllButOne(groups, "oldest");
  assert.deepEqual([...oldest.get(0)!].sort(), [1, 2]);
  assert.deepEqual(pickTotals(groups, newest), { count: 3, size: 250 });
  for (const [gi, set] of newest) assert.ok(set.size < groups[gi]!.files.length);
});

test("signed sizes", () => {
  assert.equal(signedSize(1536), "+1.5 KB");
  assert.equal(signedSize(-1024 * 1024), "-1.0 MB");
});
