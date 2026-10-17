import type {
  AppInfo,
  Batch,
  ChildrenPage,
  ChildrenQuery,
  CleanEvent,
  ComparisonView,
  Conflict,
  DeepCleanEvent,
  DriveInfo,
  FileKind,
  FindEvent,
  FindRequest,
  HealthView,
  JunkEvent,
  JunkItemsPage,
  ManualPlanView,
  NodeDetail,
  PlanView,
  PurgeReport,
  RecoveryOverview,
  RestoreReport,
  RuleSelection,
  ScanEvent,
  Settings,
  SettingsView,
  SnapshotView,
  StartupItem,
  StartupView,
  Trace,
  TraceInfo,
  TraceResult,
  TreemapNode,
  UpdateEvent,
  UpdatesStatus,
  Upgrade,
} from "./types.ts";

/** Everything the UI can ask of the backend. */
export interface Backend {
  appInfo(): Promise<AppInfo>;
  listDrives(): Promise<DriveInfo[]>;
  startScan(path: string, allowMft: boolean, onEvent: (e: ScanEvent) => void): Promise<number>;
  cancelScan(): Promise<void>;
  getNode(scanId: number, nodeId: number): Promise<NodeDetail>;
  listChildren(scanId: number, nodeId: number, query: ChildrenQuery): Promise<ChildrenPage>;
  treemap(scanId: number, nodeId: number, depth: number, maxChildren: number): Promise<TreemapNode>;
  openInExplorer(scanId: number, nodeId: number): Promise<void>;
  relaunchElevated(): Promise<void>;
  /** Shows the native folder picker. Resolves to null if cancelled. */
  pickFolder(title?: string): Promise<string | null>;
  /** Shows the native file picker. Resolves to null if cancelled. */
  pickFile(title?: string): Promise<string | null>;

  // Cleaner
  startJunkScan(onEvent: (e: JunkEvent) => void): Promise<void>;
  cancelJunkScan(): Promise<void>;
  junkItems(scanId: number, ruleId: string, offset: number, limit: number): Promise<JunkItemsPage>;
  cleanPreview(scanId: number, selection: RuleSelection[]): Promise<PlanView>;
  /** Plans items picked in the Disk view (always red tier, always to the bin). */
  cleanManual(scanId: number, nodeIds: number[]): Promise<ManualPlanView>;
  /** `secure` overwrites green and yellow files first (only when offered in Settings). */
  cleanExecute(planId: number, secure: boolean, onEvent: (e: CleanEvent) => void): Promise<void>;
  cancelClean(): Promise<void>;
  deepClean(onEvent: (e: DeepCleanEvent) => void): Promise<void>;

  // Recovery bin
  recoveryOverview(): Promise<RecoveryOverview>;
  recoveryBatch(batchId: string): Promise<Batch>;
  recoveryConflicts(batchId: string, itemIds: number[] | null): Promise<string[]>;
  recoveryRestore(batchId: string, itemIds: number[] | null, conflict: Conflict): Promise<RestoreReport>;
  recoveryDelete(batchId: string, secure: boolean): Promise<PurgeReport>;
  recoveryEmpty(secure: boolean): Promise<PurgeReport>;

  // Settings
  getSettings(): Promise<SettingsView>;
  saveSettings(settings: Settings): Promise<SettingsView>;
  excludeNode(scanId: number, nodeId: number): Promise<void>;
  openRulesFolder(): Promise<void>;

  // Files tab (large/old files and duplicates)
  startFind(path: string, request: FindRequest, rescan: boolean, onEvent: (e: FindEvent) => void): Promise<void>;
  startDupes(path: string, minSize: number, kinds: FileKind[], rescan: boolean, onEvent: (e: FindEvent) => void): Promise<void>;
  cancelFind(): Promise<void>;
  filesPlan(findId: number, indexes: number[]): Promise<ManualPlanView>;
  /** Picks are [group, file] pairs. Refused if a group would lose every copy. */
  dupesPlan(dupesId: number, picks: [number, number][]): Promise<ManualPlanView>;

  // What grew
  listSnapshots(): Promise<SnapshotView[]>;
  compareSnapshots(older: string, newer: string): Promise<ComparisonView>;
  /** Finds a path in a Disk scan; null when it isn't in that scan. */
  locateNode(scanId: number, path: string): Promise<number | null>;

  // Privacy (Windows traces)
  privacySurvey(): Promise<TraceInfo[]>;
  privacyClear(traces: Trace[]): Promise<TraceResult[]>;

  // Startup
  startupList(): Promise<StartupView>;
  /** Turns an item on or off (never deletes it) and returns it as it is now. */
  startupSetEnabled(id: string, enabled: boolean): Promise<StartupItem>;

  // App updates (winget)
  updatesStatus(): Promise<UpdatesStatus>;
  /** Asks winget for updates (can take a minute). */
  updatesList(): Promise<Upgrade[]>;
  /** Updates apps from the latest list, one at a time. */
  updatesApply(ids: string[], onEvent: (e: UpdateEvent) => void): Promise<void>;
  /** Stops checking, or stops updating after the current app. */
  updatesCancel(): Promise<void>;

  // Health check (read-only)
  healthCheck(): Promise<HealthView>;
}

let instance: Promise<Backend> | null = null;

/**
 * The backend for this build. `npm run dev:mock` swaps in fake data so the
 * UI can be developed in a plain browser; production builds never include
 * the mock (the branch is removed at build time).
 */
export function backend(): Promise<Backend> {
  instance ??=
    import.meta.env.MODE === "mock"
      ? import("./mock.ts").then((m) => m.createMockBackend())
      : import("./tauri.ts").then((m) => m.createTauriBackend());
  return instance;
}

/** Turns any thrown value into a message fit for the UI. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Something went wrong.";
}
