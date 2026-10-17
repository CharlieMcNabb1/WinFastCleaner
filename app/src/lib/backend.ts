import type {
  AppInfo,
  Batch,
  ChildrenPage,
  ChildrenQuery,
  CleanEvent,
  Conflict,
  DeepCleanEvent,
  DriveInfo,
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
  TreemapNode,
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
  cleanExecute(planId: number, onEvent: (e: CleanEvent) => void): Promise<void>;
  cancelClean(): Promise<void>;
  deepClean(onEvent: (e: DeepCleanEvent) => void): Promise<void>;

  // Recovery bin
  recoveryOverview(): Promise<RecoveryOverview>;
  recoveryBatch(batchId: string): Promise<Batch>;
  recoveryConflicts(batchId: string, itemIds: number[] | null): Promise<string[]>;
  recoveryRestore(batchId: string, itemIds: number[] | null, conflict: Conflict): Promise<RestoreReport>;
  recoveryDelete(batchId: string): Promise<PurgeReport>;
  recoveryEmpty(): Promise<PurgeReport>;

  // Settings
  getSettings(): Promise<SettingsView>;
  saveSettings(settings: Settings): Promise<SettingsView>;
  excludeNode(scanId: number, nodeId: number): Promise<void>;
  openRulesFolder(): Promise<void>;
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
