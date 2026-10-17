import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type { Backend } from "./backend.ts";
import type { CleanEvent, DeepCleanEvent, FindEvent, JunkEvent, ScanEvent, UpdateEvent } from "./types.ts";

function channel<T>(onEvent: (e: T) => void): Channel<T> {
  const c = new Channel<T>();
  c.onmessage = onEvent;
  return c;
}

export function createTauriBackend(): Backend {
  return {
    appInfo: () => invoke("app_info"),
    listDrives: () => invoke("list_drives"),
    startScan: (path, allowMft, onEvent) =>
      invoke("start_scan", { path, allowMft, onEvent: channel<ScanEvent>(onEvent) }),
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
    pickFolder: async (title = "Choose a folder to scan") => {
      const picked = await open({ directory: true, multiple: false, title });
      return typeof picked === "string" ? picked : null;
    },
    pickFile: async (title = "Choose a file") => {
      const picked = await open({ directory: false, multiple: false, title });
      return typeof picked === "string" ? picked : null;
    },

    startJunkScan: (onEvent) => invoke("start_junk_scan", { onEvent: channel<JunkEvent>(onEvent) }),
    cancelJunkScan: () => invoke("cancel_junk_scan"),
    junkItems: (scanId, ruleId, offset, limit) => invoke("junk_items", { scanId, ruleId, offset, limit }),
    cleanPreview: (scanId, selection) => invoke("clean_preview", { scanId, selection }),
    cleanManual: (scanId, nodeIds) => invoke("clean_manual", { scanId, nodeIds }),
    cleanExecute: (planId, secure, onEvent) =>
      invoke("clean_execute", { planId, secure, onEvent: channel<CleanEvent>(onEvent) }),
    cancelClean: () => invoke("cancel_clean"),
    deepClean: (onEvent) => invoke("deep_clean", { onEvent: channel<DeepCleanEvent>(onEvent) }),

    recoveryOverview: () => invoke("recovery_overview"),
    recoveryBatch: (batchId) => invoke("recovery_batch", { batchId }),
    recoveryConflicts: (batchId, itemIds) => invoke("recovery_conflicts", { batchId, itemIds }),
    recoveryRestore: (batchId, itemIds, conflict) => invoke("recovery_restore", { batchId, itemIds, conflict }),
    recoveryDelete: (batchId, secure) => invoke("recovery_delete", { batchId, secure }),
    recoveryEmpty: (secure) => invoke("recovery_empty", { secure }),

    getSettings: () => invoke("get_settings"),
    saveSettings: (settings) => invoke("save_settings", { settings }),
    excludeNode: (scanId, nodeId) => invoke("exclude_node", { scanId, nodeId }),
    openRulesFolder: () => invoke("open_rules_folder"),

    startFind: (path, request, rescan, onEvent) =>
      invoke("start_find", { path, request, rescan, onEvent: channel<FindEvent>(onEvent) }),
    startDupes: (path, minSize, kinds, rescan, onEvent) =>
      invoke("start_dupes", { path, minSize, kinds, rescan, onEvent: channel<FindEvent>(onEvent) }),
    cancelFind: () => invoke("cancel_find"),
    filesPlan: (findId, indexes) => invoke("files_plan", { findId, indexes }),
    dupesPlan: (dupesId, picks) => invoke("dupes_plan", { dupesId, picks }),

    listSnapshots: () => invoke("list_snapshots"),
    compareSnapshots: (older, newer) => invoke("compare_snapshots", { older, newer }),
    locateNode: (scanId, path) => invoke("locate_node", { scanId, path }),

    privacySurvey: () => invoke("privacy_survey"),
    privacyClear: (traces) => invoke("privacy_clear", { traces }),

    startupList: () => invoke("startup_list"),
    startupSetEnabled: (id, enabled) => invoke("startup_set_enabled", { id, enabled }),

    updatesStatus: () => invoke("updates_status"),
    updatesList: () => invoke("updates_list"),
    updatesApply: (ids, onEvent) => invoke("updates_apply", { ids, onEvent: channel<UpdateEvent>(onEvent) }),
    updatesCancel: () => invoke("updates_cancel"),

    healthCheck: () => invoke("health_check"),

    scheduleStatus: () => invoke("schedule_status"),
    scheduleSave: (schedule) => invoke("schedule_save", { schedule }),
    trayStatus: () => invoke("tray_status"),
    traySet: (enabled) => invoke("tray_set", { enabled }),
  };
}
