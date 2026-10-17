import assert from "node:assert/strict";
import { test } from "node:test";

import { category, formatDuration, formatPercent, formatSize, percent } from "./format.ts";

test("sizes use binary units with one decimal", () => {
  assert.equal(formatSize(0), "0 B");
  assert.equal(formatSize(1023), "1023 B");
  assert.equal(formatSize(1024), "1.0 KB");
  assert.equal(formatSize(1536), "1.5 KB");
  assert.equal(formatSize(4_509_715_660), "4.2 GB");
  assert.equal(formatSize(1024 * 1024 - 1), "1.0 MB");
  assert.equal(formatSize(-1), "—");
});

test("percentages", () => {
  assert.equal(percent(1, 4), 25);
  assert.equal(percent(1, 0), 0);
  assert.equal(formatPercent(0, 10), "0%");
  assert.equal(formatPercent(1, 100_000), "<0.1%");
  assert.equal(formatPercent(1, 3), "33.3%");
});

test("durations", () => {
  assert.equal(formatDuration(250), "250 ms");
  assert.equal(formatDuration(4200), "4.2 s");
  assert.equal(formatDuration(125_000), "2 min 5 s");
});

test("file categories", () => {
  assert.equal(category("movie.MKV", false), "video");
  assert.equal(category("setup.exe", false), "program");
  assert.equal(category("Windows.iso", false), "diskImage");
  assert.equal(category(".gitignore", false), "other");
  assert.equal(category("noext", false), "other");
  assert.equal(category("photos", true), "folder");
});
