<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";

  import { type Backend, errorMessage } from "../lib/backend.ts";
  import { plural, sizeAndCount } from "../lib/cleaner.ts";
  import { KIND_LABELS, KINDS, splitPath } from "../lib/files.ts";
  import { formatCount, formatDate, formatDateTime, formatSize } from "../lib/format.ts";
  import type { CleanReport, FileKind, FindView, FoundFile, ManualPlanView } from "../lib/types.ts";
  import Icon from "./Icon.svelte";
  import ManualClean from "./ManualClean.svelte";

  let {
    api,
    root,
    busy = $bindable(),
    onNavigate,
  }: {
    api: Backend;
    root: string | null;
    /** True while this panel's search runs (one search at a time). */
    busy: boolean;
    onNavigate: (tab: "recover") => void;
  } = $props();

  const MB = 1024 ** 2;
  const GB = 1024 ** 3;
  const SIZES = [
    { label: "1 MB", value: MB },
    { label: "10 MB", value: 10 * MB },
    { label: "100 MB", value: 100 * MB },
    { label: "500 MB", value: 500 * MB },
    { label: "1 GB", value: GB },
  ];
  const AGES = [
    { label: "6 months", value: 182 },
    { label: "1 year", value: 365 },
    { label: "2 years", value: 730 },
    { label: "3 years", value: 1095 },
  ];

  let mode = $state<"large" | "old">("large");
  let minSize = $state(100 * MB);
  let days = $state(365);
  const kinds = new SvelteSet<FileKind>();
  let progress = $state<{ files: number } | null>(null);
  let result = $state<FindView | null>(null);
  let files = $state<FoundFile[]>([]);
  const picked = new SvelteSet<number>();
  let error = $state<string | null>(null);
  let manual = $state<{ title: string; view: ManualPlanView } | null>(null);
  let lastRequest = $state<{ mode: "large" | "old"; minSize: number; days: number; kinds: FileKind[] } | null>(null);

  let pickedFiles = $derived(files.filter((f) => picked.has(f.index)));
  let pickedSize = $derived(pickedFiles.reduce((s, f) => s + f.size, 0));
  let allPicked = $derived(files.length > 0 && pickedFiles.length === files.length);

  function setMode(m: "large" | "old") {
    mode = m;
    minSize = m === "large" ? 100 * MB : MB;
  }

  async function search(rescan: boolean) {
    if (!root || busy) return;
    busy = true;
    error = null;
    progress = { files: 0 };
    lastRequest = { mode, minSize, days, kinds: [...kinds] };
    try {
      await api.startFind(
        root,
        {
          mode,
          minSize,
          olderThanDays: mode === "old" ? days : null,
          kinds: [...kinds],
          limit: mode === "large" ? 200 : 500,
        },
        rescan,
        (e) => {
          if (e.type === "scanning") progress = { files: e.files };
          else if (e.type === "files") {
            result = e.result;
            files = e.result.files;
            picked.clear();
            progress = null;
            busy = false;
          } else if (e.type !== "comparing") {
            progress = null;
            busy = false;
            if (e.type === "failed") error = e.message;
          }
        },
      );
    } catch (e) {
      progress = null;
      busy = false;
      error = errorMessage(e);
    }
  }

  function togglePick(f: FoundFile, on: boolean) {
    if (on) picked.add(f.index);
    else picked.delete(f.index);
  }

  function pickAll(on: boolean) {
    picked.clear();
    if (on) for (const f of files) picked.add(f.index);
  }

  async function review() {
    if (!result || pickedFiles.length === 0) return;
    try {
      const view = await api.filesPlan(result.findId, [...picked]);
      manual = { title: `Move ${plural(pickedFiles.length, "file")} to the recovery bin?`, view };
    } catch (e) {
      error = errorMessage(e);
    }
  }

  /** Moved files disappear from the list; skipped ones stay. */
  function cleaned(report: CleanReport) {
    const skipped = new Set(report.skipped.map((s) => s.path));
    files = files.filter((f) => !picked.has(f.index) || skipped.has(f.path));
    picked.clear();
  }

  let stale = $derived(
    result !== null &&
      lastRequest !== null &&
      (lastRequest.mode !== mode ||
        lastRequest.minSize !== minSize ||
        lastRequest.days !== days ||
        lastRequest.kinds.length !== kinds.size ||
        lastRequest.kinds.some((k) => !kinds.has(k))),
  );
</script>

<div class="panel">
  <div class="card controls">
    <div class="row" role="radiogroup" aria-label="What to find">
      <label class="radio"><input type="radio" name="mode" checked={mode === "large"} onchange={() => setMode("large")} /> Largest files</label>
      <label class="radio"><input type="radio" name="mode" checked={mode === "old"} onchange={() => setMode("old")} /> Not used for a long time</label>
    </div>
    <div class="row">
      <label>
        At least
        <select bind:value={minSize}>
          {#each SIZES as s (s.value)}<option value={s.value}>{s.label}</option>{/each}
        </select>
      </label>
      {#if mode === "old"}
        <label>
          Not modified or opened for
          <select bind:value={days}>
            {#each AGES as a (a.value)}<option value={a.value}>{a.label}</option>{/each}
          </select>
        </label>
      {/if}
    </div>
    <fieldset class="kinds">
      <legend class="muted">Types (none ticked: all types)</legend>
      {#each KINDS as k (k)}
        <label class="chip">
          <input type="checkbox" checked={kinds.has(k)} onchange={(e) => (e.currentTarget.checked ? kinds.add(k) : kinds.delete(k))} />
          {KIND_LABELS[k]}
        </label>
      {/each}
    </fieldset>
    <div class="row">
      <button class="btn primary" onclick={() => search(false)} disabled={!root || busy}>
        <Icon name="scan" /> Find {mode === "large" ? "large" : "old"} files
      </button>
      {#if mode === "old"}
        <span class="muted small">Windows may not record when files were opened; then the modified date is used.</span>
      {/if}
    </div>
  </div>

  {#if error}
    <div class="alert" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button class="btn ghost" aria-label="Dismiss" onclick={() => (error = null)}><Icon name="x" /></button>
    </div>
  {/if}

  {#if progress}
    <section class="card running" aria-live="polite">
      <div class="running-head">
        <strong>Scanning {root}…</strong>
        <span class="muted">{formatCount(progress.files)} files</span>
        <button class="btn" onclick={() => api.cancelFind()}><Icon name="x" /> Cancel</button>
      </div>
      <div class="progress-track" role="progressbar" aria-label="Scanning"><span class="progress-fill indeterminate"></span></div>
    </section>
  {/if}

  <ManualClean {api} bind:request={manual} {onNavigate} onDone={cleaned} onError={(m) => (error = m)} />

  {#if result}
    <p class="muted meta">
      {result.root} ·
      {#if result.reusedScan}
        using the Disk tab's scan from {formatDateTime(result.scannedAt)}
        <button class="link-btn" onclick={() => search(true)} disabled={busy}>Scan again</button>
      {:else}
        scanned {formatDateTime(result.scannedAt)}
      {/if}
      {#if stale}· <button class="link-btn" onclick={() => search(false)} disabled={busy}>Search with the new settings</button>{/if}
    </p>
    {#if files.length === 0}
      <div class="empty muted">No matching files.</div>
    {:else}
      <div class="card table-wrap">
        <table>
          <thead>
            <tr>
              <th class="pick">
                <input type="checkbox" checked={allPicked} aria-label="Pick all" onchange={(e) => pickAll(e.currentTarget.checked)} />
              </th>
              <th>Name</th>
              <th>Type</th>
              <th class="num">Size</th>
              <th class="num">Modified</th>
              <th class="num">Last used</th>
            </tr>
          </thead>
          <tbody>
            {#each files as f (f.index)}
              {@const p = splitPath(f.path)}
              <tr class:picked={picked.has(f.index)}>
                <td class="pick">
                  <input type="checkbox" checked={picked.has(f.index)} aria-label={`Pick ${p.name}`} onchange={(e) => togglePick(f, e.currentTarget.checked)} />
                </td>
                <td>
                  <div class="name">
                    <span class="ellipsis" title={f.path}>{p.name}</span>
                    <span class="muted ellipsis small" title={p.folder}>{p.folder}</span>
                  </div>
                </td>
                <td class="muted">{KIND_LABELS[f.kind]}</td>
                <td class="num">{formatSize(f.size)}</td>
                <td class="num muted" title={formatDateTime(f.modified)}>{formatDate(f.modified)}</td>
                <td class="num muted" title={formatDateTime(f.lastUsed)}>{formatDate(f.lastUsed)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
    <p class="muted small">
      {#if result.more > 0}About {plural(result.more, "more file")} not shown. {/if}
      {#if result.hidden > 0}{plural(result.hidden, "file")} in protected or excluded locations left out. {/if}
      {#if result.recentlyOpened > 0}{plural(result.recentlyOpened, "file")} left out because {result.recentlyOpened === 1 ? "it was" : "they were"} opened recently.{/if}
    </p>
    <footer class="bar">
      <span class="muted">{pickedFiles.length > 0 ? `Picked: ${sizeAndCount(pickedSize, pickedFiles.length)}` : "Nothing picked"}</span>
      <button class="btn primary" onclick={review} disabled={pickedFiles.length === 0 || busy}>
        Move {sizeAndCount(pickedSize, pickedFiles.length)} to the recovery bin…
      </button>
    </footer>
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .controls {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .row {
    display: flex;
    gap: 16px;
    align-items: center;
    flex-wrap: wrap;
  }
  .radio {
    display: inline-flex;
    gap: 6px;
    align-items: center;
  }
  select {
    font: inherit;
    color: inherit;
    padding: 4px 6px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
  }
  .kinds {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0;
    padding: 0;
    border: 0;
  }
  .kinds legend {
    font-size: 12px;
    margin-bottom: 4px;
  }
  .chip {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    padding: 2px 10px 2px 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 13px;
  }
  .running {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .running-head {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .running-head button {
    margin-left: auto;
  }
  .meta,
  .small {
    margin: 0;
    font-size: 12px;
  }
  .empty {
    padding: 30px;
    text-align: center;
  }
  .table-wrap {
    padding: 0;
    overflow: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
    min-width: 720px;
  }
  th {
    text-align: left;
    font-size: 12px;
    color: var(--text-2);
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
  }
  th.num {
    text-align: right;
  }
  th.pick,
  td.pick {
    width: 36px;
  }
  th:nth-child(3) {
    width: 100px;
  }
  th:nth-child(4) {
    width: 90px;
  }
  th:nth-child(5),
  th:nth-child(6) {
    width: 110px;
  }
  td {
    padding: 4px 8px;
    border-bottom: 1px solid var(--surface-2);
    white-space: nowrap;
  }
  tr.picked td {
    background: var(--accent-2);
  }
  .name {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .bar {
    position: sticky;
    bottom: 0;
    display: flex;
    gap: 12px;
    align-items: center;
    justify-content: flex-end;
    margin: 0 -20px -16px;
    padding: 10px 20px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
</style>
