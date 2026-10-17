import assert from "node:assert/strict";
import { test } from "node:test";

import { squarify, type Placed } from "./treemap.ts";

const rect = { x: 10, y: 20, w: 600, h: 400 };

function overlaps(a: Placed<unknown>, b: Placed<unknown>): boolean {
  const eps = 1e-6;
  return a.x + a.w > b.x + eps && b.x + b.w > a.x + eps && a.y + a.h > b.y + eps && b.y + b.h > a.y + eps;
}

test("areas are proportional and fill the rectangle", () => {
  const values = [500, 250, 120, 80, 30, 15, 5];
  const out = squarify(values.map((v, i) => ({ value: v, item: i })), rect);
  assert.equal(out.length, values.length);
  const total = values.reduce((a, b) => a + b, 0);
  let area = 0;
  for (const p of out) {
    const expected = (values[p.item]! / total) * rect.w * rect.h;
    assert.ok(Math.abs(p.w * p.h - expected) < 1e-6, `area of item ${p.item}`);
    assert.ok(p.x >= rect.x - 1e-9 && p.y >= rect.y - 1e-9);
    assert.ok(p.x + p.w <= rect.x + rect.w + 1e-6 && p.y + p.h <= rect.y + rect.h + 1e-6);
    area += p.w * p.h;
  }
  assert.ok(Math.abs(area - rect.w * rect.h) < 1e-3);
});

test("rectangles never overlap", () => {
  const out = squarify(
    Array.from({ length: 40 }, (_, i) => ({ value: 1000 / (i + 1), item: i })),
    rect,
  );
  for (let i = 0; i < out.length; i++) {
    for (let j = i + 1; j < out.length; j++) {
      assert.ok(!overlaps(out[i]!, out[j]!), `${i} overlaps ${j}`);
    }
  }
});

test("aspect ratios stay reasonable for equal items", () => {
  const out = squarify(Array.from({ length: 16 }, (_, i) => ({ value: 1, item: i })), { x: 0, y: 0, w: 400, h: 400 });
  for (const p of out) {
    const ratio = Math.max(p.w / p.h, p.h / p.w);
    assert.ok(ratio < 2.5, `ratio ${ratio}`);
  }
});

test("zero, negative, and empty inputs", () => {
  assert.deepEqual(squarify([], rect), []);
  assert.deepEqual(squarify([{ value: 0, item: 1 }], rect), []);
  const out = squarify([{ value: -5, item: 1 }, { value: 10, item: 2 }], rect);
  assert.equal(out.length, 1);
  assert.equal(out[0]!.item, 2);
  assert.deepEqual(squarify([{ value: 10, item: 1 }], { x: 0, y: 0, w: 0, h: 50 }), []);
});
