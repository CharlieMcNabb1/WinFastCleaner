<script lang="ts">
  import { onMount } from "svelte";

  import { type Backend, backend, errorMessage } from "../lib/backend.ts";
  import { formatCount, formatDuration, formatPercent, formatSize } from "../lib/format.ts";
  import type { AppInfo, DriveInfo, ManualPlanView, NodeDetail, NodeView, ScanProgress, ScanSummary } from "../lib/types.ts";
  import DrivePicker from "./DrivePicker.svelte";
  import FileList from "./FileList.svelte";
  import FlagBadges from "./FlagBadges.svelte";
  import FolderTree from "./FolderTree.svelte";
  import Icon from "./Icon.svelte";
  import ManualClean from "./ManualClean.svelte";
  import ScanProgressPanel from "./ScanProgressPanel.svelte";
  import Treemap from "./Treemap.svelte";

  let {
    info,
    target,
    onNavigate,
    onScanned,
  }: {
    info: AppInfo | null;
    /** A folder to show (from "What grew"); `seq` changes on each request. */
    target: { path: string; root: string; seq: number } | null;
    onNavigate: (tab: "recover") => void;
    /** Called when a scan finishes (it also saved a snapshot). */
    onScanned?: () => void;
  } = $props();

  let api = $state<Backend | null>(null);
  let drives = $state<DriveInfo[]>([]);
  let scanning = $state(false);
  let scanPath = $state<string | null>(null);
  let progress = $state<ScanProgress | null>(null);
  let summary = $state<ScanSummary | null>(null);
  let scanId = $state<number | null>(null);
  let root = $state<NodeDetail | null>(null);
  let current = $state<NodeDetail | null>(null);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let showSkipped = $state(false);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  /** Bumped when exclusions change, so the list re-reads its marks. */
  let refreshKey = $state(0);
  let manual = $state<{ title: string; view: ManualPlanView } | null>(null);
  /** A path to open once the scan that is running finishes. */
  let pendingReveal: string | null = null;
  let lastTarget = 0;

  $effect(() => {
    const t = target;
    if (t && t.seq !== lastTarget) {
      lastTarget = t.seq;
      void reveal(t.path, t.root);
    }
  });

  let lineage = $derived(new Set(current?.lineage.map((c) => c.id) ?? []));
  let isWindows = $derived(info?.platform === "windows");

  onMount(() => {
    void init();
    return () => clearTimeout(noticeTimer);
  });

  async function init() {
    api = await backend();
    await refreshDrives();
    // First launch: no setup wizard, just scan the system drive.
    const system = drives.find((d) => d.isSystem) ?? drives[0];
    if (system) await startScan(system.root);
  }

  async function refreshDrives() {
    if (!api) return;
    try {
      drives = await api.listDrives();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function startScan(path: string) {
    if (!api || scanning) return;
    error = null;
    scanning = true;
    scanPath = path;
    progress = null;
    try {
      await api.startScan(path, true, (e) => {
        switch (e.type) {
          case "progress":
            progress = e;
            break;
          case "done":
            scanning = false;
            summary = e.summary;
            scanId = e.scanId;
            showSkipped = false;
            void open(0, true).then(() => {
              const p = pendingReveal;
              pendingReveal = null;
              if (p) void reveal(p, null);
            });
            void refreshDrives();
            onScanned?.();
            break;
          case "cancelled":
            scanning = false;
            progress = null;
            break;
          case "failed":
            scanning = false;
            error = e.message;
            break;
        }
      });
    } catch (e) {
      scanning = false;
      error = errorMessage(e);
    }
  }

  async function open(id: number, isRoot = false) {
    if (!api || scanId === null) return;
    try {
      const detail = await api.getNode(scanId, id);
      current = detail;
      if (isRoot) root = detail;
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function scanFolder() {
    if (!api) return;
    try {
      const folder = await api.pickFolder();
      if (folder) await startScan(folder);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function restartAsAdmin() {
    if (!api) return;
    try {
      await api.relaunchElevated();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  /** "Send to cleaner": items picked by hand are always red tier and always
   *  go to the recovery bin. The backend builds and checks the exact list. */
  async function sendToCleaner(node: NodeView) {
    if (!api || scanId === null) return;
    try {
      manual = { title: `Move "${node.name}" to the recovery bin?`, view: await api.cleanManual(scanId, [node.id]) };
    } catch (e) {
      error = errorMessage(e);
    }
  }

  /** Opens `path` in the current scan, scanning `root` first if needed. */
  async function reveal(path: string, root: string | null) {
    api ??= await backend();
    if (scanId !== null) {
      try {
        const id = await api.locateNode(scanId, path);
        if (id !== null) {
          await open(id);
          return;
        }
      } catch {
        // Not in this scan; scan its root below.
      }
    }
    if (root && !scanning) {
      pendingReveal = path;
      await startScan(root);
    } else if (!root) {
      showNotice("That folder isn't in this scan anymore.");
    }
  }

  async function exclude(node: NodeView) {
    if (!api || scanId === null) return;
    try {
      await api.excludeNode(scanId, node.id);
      refreshKey += 1;
      showNotice(`"${node.name}" added to exclusions`);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function showNotice(message: string) {
    notice = message;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), 3000);
  }
</script>

<section class="disk">
  <header class="toolbar">
    <div class="title">
      <h1>Disk</h1>
      <p class="muted">See what takes up space. Scanning only reads. Nothing is changed or deleted.</p>
    </div>
    <div class="controls">
      <DrivePicker {drives} activeRoot={summary?.root ?? null} disabled={scanning || !api} onScan={startScan} />
      <button class="btn" onclick={scanFolder} disabled={scanning || !api}>
        <Icon name="folderOpen" /> Scan a folder…
      </button>
    </div>
  </header>

  {#if isWindows && info && !info.elevated}
    <div class="notice">
      <Icon name="shield" size={18} />
      <span>
        <strong>Fast scan needs administrator rights.</strong>
        Reading the NTFS Master File Table scans a whole drive in seconds. Without admin rights, a standard folder
        scan is used instead.
      </span>
      <button class="btn" onclick={restartAsAdmin}><Icon name="shield" /> Restart as administrator</button>
    </div>
  {/if}

  {#if scanning}
    <ScanProgressPanel path={scanPath} {progress} canBrowse={summary !== null} onCancel={() => api?.cancelScan()} />
  {/if}

  {#if error}
    <div class="alert" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button class="btn ghost" aria-label="Dismiss" onclick={() => (error = null)}><Icon name="x" /></button>
    </div>
  {/if}

  {#if api}
    <ManualClean
      {api}
      bind:request={manual}
      {onNavigate}
      onDone={() => showNotice("Scan again to update the sizes shown here.")}
      onError={(m) => (error = m)}
    />
  {/if}

  {#if summary && scanId !== null && api && root && current}
    <div class="summary">
      <span>
        <strong>{summary.root}</strong>
        · {formatSize(summary.size)}
        <span class="muted">({formatSize(summary.allocated)} on disk)</span>
        · {formatCount(summary.files)} files · {formatCount(summary.dirs)} folders
      </span>
      <span class="muted">
        Scanned in {formatDuration(summary.durationMs)} ·
        <span class="method" title={summary.mftUnavailable ?? ""}>
          {summary.method === "mft" ? "Fast scan (MFT)" : "Standard scan"}
        </span>
      </span>
      {#if summary.skipped > 0}
        <button class="link" onclick={() => (showSkipped = !showSkipped)} aria-expanded={showSkipped}>
          <Icon name="alert" size={14} />
          {formatCount(summary.skipped)} item{summary.skipped === 1 ? "" : "s"} couldn't be read
        </button>
      {/if}
    </div>
    {#if showSkipped}
      <ul class="skipped">
        {#each summary.skippedDetails as s (s.path + s.reason)}
          <li><span class="path">{s.path}</span> <span class="muted">{s.reason}</span></li>
        {/each}
        {#if summary.skipped > summary.skippedDetails.length}
          <li class="muted">…and {formatCount(summary.skipped - summary.skippedDetails.length)} more</li>
        {/if}
      </ul>
    {/if}

    <nav class="crumbs" aria-label="Current folder">
      {#each current.lineage as crumb, i (crumb.id)}
        {#if i > 0}<Icon name="chevronRight" size={12} />{/if}
        <button class="crumb" aria-current={crumb.id === current.id ? "location" : undefined} onclick={() => open(crumb.id)}>
          {crumb.name}
        </button>
      {/each}
      <span class="here muted">
        {formatSize(current.size)}
        {#if summary.volume}· {formatPercent(current.size, summary.volume.total)} of drive{/if}
        <FlagBadges flags={current.flags} />
      </span>
      {#if current.lineage.length > 1}
        <button class="btn ghost up" onclick={() => open(current!.lineage.at(-2)!.id)} aria-label="Up one level">
          <Icon name="up" />
        </button>
      {/if}
    </nav>

    <div class="panes">
      <aside class="tree-pane" aria-label="Folder tree">
        <FolderTree {api} {scanId} {root} currentId={current.id} {lineage} onOpen={(id) => open(id)} />
      </aside>
      <div class="main-pane">
        <div class="map-pane">
          <Treemap {api} {scanId} nodeId={current.id} onOpen={(id) => open(id)} />
        </div>
        <div class="list-pane">
          <FileList
            {api}
            {scanId}
            nodeId={current.id}
            folderSize={current.size}
            driveTotal={summary.volume?.total ?? null}
            canReveal={isWindows}
            {refreshKey}
            onOpen={(id) => open(id)}
            onNotice={showNotice}
            onSendToCleaner={sendToCleaner}
            onExclude={exclude}
          />
        </div>
      </div>
    </div>
  {:else if !scanning}
    <div class="empty muted">Pick a drive above to see what is taking up space.</div>
  {/if}

  {#if notice}
    <div class="toast" role="status">{notice}</div>
  {/if}
</section>


<style>
  .disk {
    display: flex;
    flex-direction: column;
    gap: 12px;
    height: 100%;
    padding: 16px 20px;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .title h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 600;
  }
  .title p {
    margin: 2px 0 0;
  }
  .controls {
    display: flex;
    gap: 10px;
    align-items: stretch;
    flex-wrap: wrap;
  }
  .notice,
  .alert {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 10px 12px;
    border-radius: var(--radius);
    border: 1px solid var(--notice-border);
    background: var(--notice-bg);
  }
  .notice span,
  .alert span {
    flex: 1;
  }
  .alert {
    border-color: var(--alert-border);
    background: var(--alert-bg);
  }
  .summary {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    align-items: center;
  }
  .method {
    text-decoration: underline dotted;
  }
  .link {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    border: 0;
    background: none;
    color: var(--accent);
    padding: 0;
  }
  .skipped {
    margin: 0;
    padding: 8px 12px 8px 28px;
    max-height: 140px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    font-size: 12px;
  }
  .path {
    font-family: var(--mono);
  }
  .crumbs {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-wrap: wrap;
    color: var(--text-2);
  }
  .crumb {
    border: 0;
    background: none;
    padding: 2px 4px;
    border-radius: 4px;
    color: var(--text);
  }
  .crumb:hover {
    background: var(--surface-2);
  }
  .crumb[aria-current] {
    font-weight: 600;
  }
  .here {
    margin-left: 8px;
    display: inline-flex;
    align-items: center;
  }
  .up {
    margin-left: auto;
  }
  .panes {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(240px, 320px) 1fr;
    gap: 12px;
  }
  .tree-pane {
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  .main-pane {
    display: grid;
    grid-template-rows: minmax(160px, 44%) 1fr;
    gap: 12px;
    min-height: 0;
    min-width: 0;
  }
  .map-pane,
  .list-pane {
    min-height: 0;
    min-width: 0;
  }
  .empty {
    flex: 1;
    display: grid;
    place-items: center;
  }
  .toast {
    position: fixed;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    padding: 8px 14px;
    border-radius: var(--radius);
    background: var(--text);
    color: var(--bg);
    box-shadow: var(--shadow);
  }
</style>
