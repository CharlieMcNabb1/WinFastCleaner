// The latest junk scan, shared by Home and the Cleaner tab so one scan
// serves both.

import { backend, errorMessage } from "./backend.ts";
import type { JunkView } from "./types.ts";

export const junkScan = $state<{ view: JunkView | null; scanning: boolean; error: string | null }>({
  view: null,
  scanning: false,
  error: null,
});

/** Starts a junk scan unless one is running. */
export async function scanJunk(): Promise<void> {
  if (junkScan.scanning) return;
  junkScan.scanning = true;
  junkScan.error = null;
  try {
    const api = await backend();
    await api.startJunkScan((e) => {
      junkScan.scanning = false;
      if (e.type === "done") junkScan.view = e.result;
      else if (e.type === "failed") junkScan.error = e.message;
    });
  } catch (e) {
    junkScan.scanning = false;
    junkScan.error = errorMessage(e);
  }
}

export async function cancelJunkScan(): Promise<void> {
  await (await backend()).cancelJunkScan();
}

/** What "Clean safe items" would remove: green rules that are ready. */
export function safeTotals(view: JunkView): { size: number; count: number } {
  let size = 0;
  let count = 0;
  for (const r of view.rules) {
    if (r.tier === "green" && r.status.kind === "ready") {
      size += r.size;
      count += r.files;
    }
  }
  return { size, count };
}
