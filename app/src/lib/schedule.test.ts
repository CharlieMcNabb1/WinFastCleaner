import assert from "node:assert/strict";
import { test } from "node:test";

import { describe, nextRun, parseTime, timeValue } from "./schedule.ts";
import type { Schedule } from "./types.ts";

const base: Schedule = {
  enabled: true,
  frequency: "weekly",
  weekday: "sunday",
  hour: 12,
  minute: 0,
  includeOptional: false,
  rules: [],
};

// Wednesday, Oct 14, 2026, 10:30 local time.
const wed = new Date(2026, 9, 14, 10, 30);

test("daily runs are later today, or tomorrow once the time has passed", () => {
  const daily = { ...base, frequency: "daily" as const };
  assert.equal(wed.getDay(), 3);
  assert.deepEqual(nextRun(daily, wed), new Date(2026, 9, 14, 12, 0));
  assert.deepEqual(nextRun({ ...daily, hour: 9 }, wed), new Date(2026, 9, 15, 9, 0));
  // Exactly at the time counts as passed.
  assert.deepEqual(nextRun({ ...daily, hour: 10, minute: 30 }, wed), new Date(2026, 9, 15, 10, 30));
});

test("weekly runs land on the chosen weekday", () => {
  assert.deepEqual(nextRun(base, wed), new Date(2026, 9, 18, 12, 0));
  assert.deepEqual(nextRun({ ...base, weekday: "wednesday" }, wed), new Date(2026, 9, 14, 12, 0));
  assert.deepEqual(nextRun({ ...base, weekday: "wednesday", hour: 8 }, wed), new Date(2026, 9, 21, 8, 0));
  assert.deepEqual(nextRun({ ...base, weekday: "monday" }, wed), new Date(2026, 9, 19, 12, 0));
});

test("times parse and print", () => {
  assert.deepEqual(parseTime("07:05"), { hour: 7, minute: 5 });
  assert.deepEqual(parseTime("7:05"), { hour: 7, minute: 5 });
  assert.equal(parseTime("24:00"), null);
  assert.equal(parseTime("12:60"), null);
  assert.equal(parseTime("noon"), null);
  assert.equal(timeValue({ ...base, hour: 7, minute: 5 }), "07:05");
});

test("descriptions say when and what", () => {
  assert.equal(describe(base), "Every Sunday at 12:00, safe items only");
  assert.equal(
    describe({ ...base, frequency: "daily", includeOptional: true, rules: ["a", "b"] }),
    "Every day at 12:00, safe and optional items, 2 chosen rules",
  );
});
