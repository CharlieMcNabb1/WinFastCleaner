// Pure helpers for the scheduled cleaning settings (kept out of components
// so they can be unit tested).

import type { Schedule, Weekday } from "./types.ts";

export const WEEKDAYS: { id: Weekday; label: string }[] = [
  { id: "monday", label: "Monday" },
  { id: "tuesday", label: "Tuesday" },
  { id: "wednesday", label: "Wednesday" },
  { id: "thursday", label: "Thursday" },
  { id: "friday", label: "Friday" },
  { id: "saturday", label: "Saturday" },
  { id: "sunday", label: "Sunday" },
];

/** JavaScript's day number (Sunday = 0) for a weekday. */
function dayNumber(d: Weekday): number {
  return (WEEKDAYS.findIndex((w) => w.id === d) + 1) % 7;
}

/** When the schedule runs next after `now`, in local time. */
export function nextRun(s: Schedule, now: Date): Date {
  const next = new Date(now);
  next.setHours(s.hour, s.minute, 0, 0);
  if (s.frequency === "daily") {
    if (next <= now) next.setDate(next.getDate() + 1);
    return next;
  }
  let days = (dayNumber(s.weekday) - next.getDay() + 7) % 7;
  if (days === 0 && next <= now) days = 7;
  next.setDate(next.getDate() + days);
  return next;
}

/** "HH:MM" for a time input. */
export function timeValue(s: Schedule): string {
  return `${String(s.hour).padStart(2, "0")}:${String(s.minute).padStart(2, "0")}`;
}

/** Parses a time input's value; null when it isn't a valid time. */
export function parseTime(v: string): { hour: number; minute: number } | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(v.trim());
  if (!m) return null;
  const hour = Number(m[1]);
  const minute = Number(m[2]);
  return hour <= 23 && minute <= 59 ? { hour, minute } : null;
}

/** A short description such as "Every Sunday at 12:00, safe items only". */
export function describe(s: Schedule): string {
  const when =
    s.frequency === "daily" ? "Every day" : `Every ${WEEKDAYS.find((w) => w.id === s.weekday)?.label ?? s.weekday}`;
  const what = s.includeOptional ? "safe and optional items" : "safe items only";
  const rules = s.rules.length > 0 ? `, ${s.rules.length} chosen rule${s.rules.length === 1 ? "" : "s"}` : "";
  return `${when} at ${timeValue(s)}, ${what}${rules}`;
}
