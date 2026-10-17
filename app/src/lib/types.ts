// Types shared with the Rust backend (see app/src-tauri/src/commands.rs).

export interface AppInfo {
  name: string;
  version: string;
  platform: string;
  elevated: boolean;
}

export type DriveKind = "fixed" | "removable" | "network" | "optical" | "ramDisk" | "unknown";

export interface DriveInfo {
  root: string;
  label: string;
  fileSystem: string;
  kind: DriveKind;
  total: number;
  free: number;
  isSystem: boolean;
}

export type ScanPhase = "starting" | "walking" | "readingMft" | "building";

export interface ScanProgress {
  phase: ScanPhase;
  files: number;
  dirs: number;
  bytes: number;
  done: number;
  total: number;
  current: string | null;
}

export interface Skipped {
  path: string;
  reason: string;
}

export interface ScanSummary {
  root: string;
  method: "mft" | "walk";
  mftUnavailable: string | null;
  startedAt: number;
  durationMs: number;
  size: number;
  allocated: number;
  files: number;
  dirs: number;
  skipped: number;
  skippedDetails: Skipped[];
  volume: { total: number; free: number } | null;
}

export type ScanEvent =
  | ({ type: "progress" } & ScanProgress)
  | { type: "done"; scanId: number; summary: ScanSummary; snapshotSaved: boolean }
  | { type: "cancelled"; scanId: number }
  | { type: "failed"; scanId: number; message: string };

export interface NodeView {
  id: number;
  name: string;
  isDir: boolean;
  size: number;
  allocated: number;
  files: number;
  dirs: number;
  modified: number;
  flags: number;
  childCount: number;
  /** The safety guard never lets the cleaner touch this item. */
  protected: boolean;
  /** On the user's exclusions list. */
  excluded: boolean;
}

export interface NodeDetail extends NodeView {
  path: string;
  lineage: { id: number; name: string }[];
}

export type SortKey = "size" | "allocated" | "name" | "modified" | "files";

export interface ChildrenQuery {
  offset: number;
  limit: number;
  sort: SortKey;
  descending: boolean;
  dirsOnly: boolean;
}

export interface ChildrenPage {
  total: number;
  items: NodeView[];
  restCount: number;
  restSize: number;
}

export interface TreemapNode {
  id: number;
  name: string;
  size: number;
  isDir: boolean;
  flags: number;
  children: TreemapNode[];
  otherSize: number;
  otherCount: number;
}

/** Node flag bits (mirror of `cleaner_core::tree::flags`). */
export const Flags = {
  DIR: 1 << 0,
  LINK: 1 << 1,
  HARDLINK: 1 << 2,
  HARDLINK_DUP: 1 << 3,
  HAS_HARDLINKS: 1 << 4,
  UNREADABLE: 1 << 5,
  METAFILE: 1 << 6,
  CLOUD: 1 << 7,
  HIDDEN: 1 << 8,
  SYSTEM: 1 << 9,
  HAS_UNREADABLE: 1 << 10,
} as const;

// ----------------------------------------------------------------- cleaner
// (see app/src-tauri/src/cleaning.rs)

export type Tier = "green" | "yellow" | "red";

export type RuleStatus =
  | { kind: "ready" }
  | { kind: "needsAdmin" }
  | { kind: "appRunning"; detail: string[] }
  | { kind: "nothingFound" };

export interface RuleView {
  id: string;
  name: string;
  group: string;
  tier: Tier;
  description: string;
  lose: string;
  requiresAdmin: boolean;
  closeApps: string[];
  minAgeDays: number | null;
  userRule: boolean;
  status: RuleStatus;
  files: number;
  size: number;
  hidden: number;
}

export interface RuleError {
  source: string;
  rule: string | null;
  message: string;
}

export interface JunkView {
  scanId: number;
  rules: RuleView[];
  errors: RuleError[];
  scannedAt: number;
  durationMs: number;
  elevated: boolean;
}

export type JunkEvent =
  | { type: "done"; result: JunkView }
  | { type: "cancelled" }
  | { type: "failed"; message: string };

export interface JunkItem {
  index: number;
  path: string;
  size: number;
  modified: number;
}

export interface JunkItemsPage {
  total: number;
  items: JunkItem[];
}

export interface RuleSelection {
  ruleId: string;
  /** Indexes of files unticked in the preview. */
  deselected: number[];
}

export interface PlanView {
  planId: number;
  count: number;
  totalSize: number;
  deleteCount: number;
  deleteBytes: number;
  binCount: number;
  binBytes: number;
  notReady: { ruleId: string; status: RuleStatus }[];
}

export interface ManualPlanView {
  plan: PlanView;
  sample: { path: string; size: number }[];
  rejected: { path: string; reason: string }[];
}

export interface PurgeReport {
  batches: number;
  files: number;
  bytes: number;
  failed: number;
}

export interface CleanReport {
  deletedCount: number;
  deletedBytes: number;
  /** Of the deleted files, how many were overwritten first. */
  wipedCount: number;
  binnedCount: number;
  binnedBytes: number;
  skippedCount: number;
  skipped: { path: string; reason: string }[];
  freedBytes: number;
  batchId: string | null;
  evicted: PurgeReport;
  foldersRemoved: number;
}

export type CleanEvent =
  | { type: "progress"; done: number; total: number }
  | { type: "done"; report: CleanReport }
  | { type: "failed"; message: string };

export type DeepCleanEvent =
  | { type: "progress"; percent: number }
  | { type: "done"; freedBytes: number; restartNeeded: boolean }
  | { type: "failed"; message: string };

// ----------------------------------------------------------------- recovery
// (see app/src-tauri/src/recover.rs)

export interface BatchSummary {
  id: string;
  createdAt: number;
  label: string;
  items: number;
  size: number;
}

export interface BinItem {
  id: number;
  original: string;
  stored: string;
  size: number;
  tier: Tier;
  rule: string | null;
  state: "pending" | "inBin";
}

export interface Batch {
  id: string;
  createdAt: number;
  label: string;
  items: BinItem[];
}

export interface DriveUsage {
  drive: string;
  bytes: number;
  limit: number;
}

export interface BinLimits {
  maxBytes: number;
  maxPercent: number;
  maxBatches: number;
  maxAgeDays: number;
}

export interface RecoveryOverview {
  batches: BatchSummary[];
  usage: DriveUsage[];
  limits: BinLimits;
}

export type Conflict = "skip" | "keepBoth" | "overwrite";

export interface RestoreReport {
  restored: number;
  restoredBytes: number;
  renamed: string[];
  skipped: [string, string][];
}

// ----------------------------------------------------------------- settings
// (see app/src-tauri/src/settings_cmds.rs)

export type ExclusionKind = "folder" | "file" | "pattern";

export interface Exclusion {
  kind: ExclusionKind;
  value: string;
}

export interface Settings {
  exclusions: Exclusion[];
  greenToBin: boolean;
  bin: BinLimits;
  /** Keep scan snapshots ("What grew") this many days. */
  snapshotDays: number;
  /** Offer secure delete (overwrite before deleting). */
  secureDelete: boolean;
  /** When health checks turn yellow or red. */
  health: HealthThresholds;
  /** Changed only through scheduleSave (it also updates the task). */
  schedule: Schedule;
  /** Changed only through traySet (it also updates the sign-in entry). */
  tray: boolean;
  theme: Theme;
}

export type Theme = "system" | "light" | "dark";

export interface SettingsView {
  settings: Settings;
  dataDir: string;
  rulesDir: string;
  userRules: number;
  ruleErrors: RuleError[];
}

// ----------------------------------------------------------------- files
// (see app/src-tauri/src/files.rs)

export type FileKind = "video" | "audio" | "image" | "archive" | "installer" | "diskImage" | "document" | "other";

export interface FindRequest {
  mode: "large" | "old";
  minSize: number;
  olderThanDays: number | null;
  kinds: FileKind[];
  limit: number;
}

export interface FoundFile {
  index: number;
  path: string;
  size: number;
  modified: number;
  lastUsed: number;
  kind: FileKind;
}

export interface FindView {
  findId: number;
  root: string;
  reusedScan: boolean;
  scannedAt: number;
  files: FoundFile[];
  more: number;
  hidden: number;
  recentlyOpened: number;
}

export interface DupeFileView {
  index: number;
  path: string;
  modified: number;
}

export interface DupeGroupView {
  index: number;
  size: number;
  hash: string;
  wasted: number;
  files: DupeFileView[];
}

export interface DupesView {
  dupesId: number;
  root: string;
  reusedScan: boolean;
  scannedAt: number;
  groups: DupeGroupView[];
  wasted: number;
  filesHashed: number;
  bytesHashed: number;
  unreadable: number;
  hidden: number;
}

export type DupePhase = "sizes" | "sampling" | "hashing";

export type FindEvent =
  | { type: "scanning"; files: number; bytes: number }
  | { type: "comparing"; phase: DupePhase; done: number; total: number }
  | { type: "files"; result: FindView }
  | { type: "dupes"; result: DupesView }
  | { type: "cancelled" }
  | { type: "failed"; message: string };

// ----------------------------------------------------------------- what grew
// (see app/src-tauri/src/grew_cmds.rs)

export interface SnapshotView {
  key: string;
  root: string;
  takenAt: number;
  size: number;
}

export interface ChangeView {
  path: string;
  oldSize: number | null;
  newSize: number | null;
  delta: number;
  rule: { id: string; name: string } | null;
}

export interface ComparisonView {
  root: string;
  oldTakenAt: number;
  newTakenAt: number;
  totalDelta: number;
  threshold: number;
  grew: ChangeView[];
  shrank: ChangeView[];
}

// ----------------------------------------------------------------- privacy
// (see app/src-tauri/src/privacy_cmds.rs)

export type Trace = "recentDocuments" | "jumpLists" | "runHistory" | "clipboard";

export interface TraceInfo {
  trace: Trace;
  name: string;
  description: string;
  lose: string;
  items: number | null;
  available: boolean;
}

export interface TraceResult {
  trace: Trace;
  cleared: number;
  skipped: { path: string; reason: string }[];
  error: string | null;
}

// ----------------------------------------------------------------- startup
// (see crates/core/src/startup.rs and app/src-tauri/src/startup_cmds.rs)

export type StartupSource = "userRun" | "machineRun" | "machineRun32" | "userFolder" | "commonFolder" | "task";

export interface StartupItem {
  id: string;
  name: string;
  source: StartupSource;
  location: string;
  command: string;
  program: string | null;
  publisher: string | null;
  enabled: boolean;
  /** Starts for every user; changing it needs administrator rights. */
  machine: boolean;
  disabledAt: number | null;
}

export interface ServiceItem {
  name: string;
  displayName: string;
  program: string | null;
  publisher: string | null;
  running: boolean;
  delayed: boolean;
}

export interface StartupView {
  items: StartupItem[];
  errors: string[];
  services: ServiceItem[];
  servicesError: string | null;
  elevated: boolean;
}

// ----------------------------------------------------------------- updates
// (see crates/core/src/winget.rs and app/src-tauri/src/updates_cmds.rs)

export interface Upgrade {
  name: string;
  id: string;
  version: string;
  available: string;
  source: string | null;
  /** False when winget's id was cut short, so it can't be passed back. */
  canUpdate: boolean;
}

export interface UpgradeResult {
  id: string;
  ok: boolean;
  message: string;
}

export interface UpdatesSummary {
  checkedAt: number;
  count: number;
}

export interface UpdatesStatus {
  installed: boolean;
  missingMessage: string | null;
  last: UpdatesSummary | null;
}

export type UpdateEvent =
  | { type: "started"; id: string; index: number; total: number }
  | { type: "finished"; result: UpgradeResult }
  | { type: "done"; cancelled: boolean };

// ----------------------------------------------------------------- health
// (see crates/core/src/health.rs and app/src-tauri/src/health_cmds.rs)

export type HealthStatus = "unknown" | "green" | "yellow" | "red";
export type CheckId = "diskSpace" | "junk" | "startup" | "driveHealth" | "temperatures" | "updates" | "recoveryBin";
/** The tab where a check's problem can be fixed. */
export type Fix = "disk" | "cleaner" | "startup" | "updates" | "recover";

export interface Check {
  id: CheckId;
  title: string;
  status: HealthStatus;
  summary: string;
  details: string[];
  fix: Fix | null;
  asOf: number | null;
}

export interface HealthView {
  checks: Check[];
  overall: HealthStatus;
}

export interface HealthThresholds {
  diskFreeYellowPct: number;
  diskFreeRedPct: number;
  junkYellowGb: number;
  junkRedGb: number;
  startupYellow: number;
  startupRed: number;
  binYellowPct: number;
  ssdWearYellowPct: number;
}

// ----------------------------------------------------------------- navigation

/** Where the Cleaner tab should go when opened from another tab. */
export type CleanerFocus = { kind: "rule"; ruleId: string; seq: number } | { kind: "safe"; seq: number };

// ----------------------------------------------------------------- automation
// (see crates/core/src/schedule.rs and app/src-tauri/src/automation_cmds.rs)

export type Frequency = "daily" | "weekly";
export type Weekday = "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday";

export interface Schedule {
  enabled: boolean;
  frequency: Frequency;
  weekday: Weekday;
  hour: number;
  minute: number;
  /** Also clean optional (yellow) items; they go to the recovery bin. */
  includeOptional: boolean;
  /** Only these rules; empty means every rule in the chosen tiers. */
  rules: string[];
}

export interface LastRun {
  startedAt: number;
  finishedAt: number;
  ok: boolean;
  message: string;
  deletedCount: number;
  deletedBytes: number;
  binnedCount: number;
  binnedBytes: number;
  skippedCount: number;
  freedBytes: number;
  batchId: string | null;
  notReady: { rule: string; reason: string }[];
}

export interface RuleChoice {
  id: string;
  name: string;
  group: string;
  tier: Tier;
}

export interface ScheduleView {
  schedule: Schedule;
  /** Whether the Task Scheduler task exists (null when it can't be read). */
  taskExists: boolean | null;
  programFound: boolean;
  last: LastRun | null;
  rules: RuleChoice[];
}

export interface TrayView {
  enabled: boolean;
  running: boolean;
  programFound: boolean;
}
