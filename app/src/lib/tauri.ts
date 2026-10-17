import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type { Backend } from "./backend.ts";
import type { ScanEvent } from "./types.ts";

export function createTauriBackend(): Backend {
  return {
    appInfo: () => invoke("app_info"),
    listDrives: () => invoke("list_drives"),
    startScan: (path, allowMft, onEvent) => {
      const channel = new Channel<ScanEvent>();
      channel.onmessage = onEvent;
      return invoke("start_scan", { path, allowMft, onEvent: channel });
    },
    cancelScan: () => invoke("cancel_scan"),
    getNode: (scanId, nodeId) => invoke("get_node", { scanId, nodeId }),
    listChildren: (scanId, nodeId, q) =>
      invoke("list_children", {
        scanId,
        nodeId,
        offset: q.offset,
        limit: q.limit,
        sort: q.sort,
        descending: q.descending,
        dirsOnly: q.dirsOnly,
      }),
    treemap: (scanId, nodeId, depth, maxChildren) =>
      invoke("treemap", { scanId, nodeId, depth, maxChildren }),
    openInExplorer: (scanId, nodeId) => invoke("open_in_explorer", { scanId, nodeId }),
    relaunchElevated: () => invoke("relaunch_elevated"),
    pickFolder: async () => {
      const picked = await open({ directory: true, multiple: false, title: "Choose a folder to scan" });
      return typeof picked === "string" ? picked : null;
    },
  };
}
