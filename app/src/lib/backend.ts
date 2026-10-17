import type {
  AppInfo,
  ChildrenPage,
  ChildrenQuery,
  DriveInfo,
  NodeDetail,
  ScanEvent,
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
  pickFolder(): Promise<string | null>;
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
