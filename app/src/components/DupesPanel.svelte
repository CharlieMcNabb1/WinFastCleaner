<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";

  import { type Backend, errorMessage } from "../lib/backend.ts";
  import { plural, sizeAndCount } from "../lib/cleaner.ts";
  import { type DupePicks, KIND_LABELS, KINDS, canPick, pickAllButOne, pickTotals, splitPath } from "../lib/files.ts";
  import { formatDate, formatDateTime, formatSize, percent } from "../lib/format.ts";
  import type { CleanReport, DupeGroupView, DupePhase, DupesView, FileKind, ManualPlanView } from "../lib/types.ts";
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
    busy: boolean;
    onNavigate: (tab: "recover") => void;
  } = $props();

  const MB = 1024 ** 2;
  const SIZES = [
    { label: "1 MB", value: MB },
    { label: "10 MB", value: 10 * MB },
    { label: "100 MB", value: 100 * MB },
    { label: "1 GB", value: 1024 * MB },
  ];

  let minSize = $state(MB);
  const kinds = new SvelteSet<FileKind>();
  let progress = $state<{ phase: DupePhase | "scanning"; done: number; total: number } | null>(null);
  let result = $state<DupesView | null>(null);
  let groups = $state<DupeGroupView[]>([]);
  let picks = $state<DupePicks>(new Map());
  let error = $state<string | null>(null);
  let manual = $state<{ title: string; view: ManualPlanView } | null>(null);

  let totals = $derived(pickTotals(groups, picks));

  async function search(rescan: boolean) {
    if (!root || busy) return;
    busy = true;
    error = null;
    progress = { phase: "scanning", done: 0, total: 0 };
    try {
      await api.startDupes(root, minSize, [...kinds], rescan, (e) => {
        if (e.type === "scanning") progress = { phase: "scanning", done: e.files, total: 0 };
        else if (e.type === "comparing") progress = { phase: e.phase, done: e.done, total: e.total };
        else if (e.type === "dupes") {
          result = e.result;
          groups = e.result.groups;
          picks = new Map();
          progress = null;
          busy = false;
        } else {
          progress = null;
          busy = false;
          if (e.type === "failed") error = e.message;
        }
      });
    } catch (e) {
      progress = null;
      busy = false;
      error = errorMessage(e);
    }
  }

  function toggle(g: DupeGroupView, file: number, on: boolean) {
    const next = new Map(picks);
    const set = new Set(next.get(g.index) ?? []);
    if (on) {
      if (!canPick(g, picks, file)) return;
      set.add(file);
    } else set.delete(file);
    next.set(g.index, set);
    picks = next;
  }

  async function review() {
    if (!result || totals.count === 0) return;
    const pairs: [number, number][] = [];
    for (const [g, files] of picks) for (const f of files) pairs.push([g, f]);
    try {
      const view = await api.dupesPlan(result.dupesId, pairs);
      manual = { title: `Move ${plural(totals.count, "extra copy", "extra copies")} to the recovery bin?`, view };
    } catch (e) {
      error = errorMessage(e);
    }
  }

  /** Moved copies disappear; groups left with one copy disappear too. */
  function cleaned(report: CleanReport) {
    const skipped = new Set(report.skipped.map((s) => s.path));
    groups = groups
      .map((g) => ({
        ...g,
        files: g.files.filter((f) => !picks.get(g.index)?.has(f.index) || skipped.has(f.path)),
      }))
      .filter((g) => g.files.length > 1);
    picks = new Map();
  }

  function phaseText(p: NonNullable<typeof progress>): string {
    switch (p.phase) {
      case "scanning":
        return `Scanning ${root}… ${p.done.toLocaleString()} files`;
      case "sizes":
        return "Comparing file sizes…";
      case "sampling":
        return `Checking ${p.done.toLocaleString()} of ${p.total.toLocaleString()} possible matches…`;
      case "hashing":
        return `Comparing contents: ${formatSize(p.done)} of ${formatSize(p.total)}`;
    }
  }
</script>

<div class="panel">
  <div class="card controls">
    <div class="row">
      <label>
        Skip files under
        <select bind:value={minSize}>
          {#each SIZES as s (s.value)}<option value={s.value}>{s.label}</option>{/each}
        </select>
      </label>
      <span class="muted small">Files are compared by size, then by their full contents (BLAKE3 hash).</span>
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
      <button class="btn primary" onclick={() => search(false)} disabled={!root || busy}><Icon name="layers" /> Find duplicates</button>
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
        <strong>{phaseText(progress)}</strong>
        <button class="btn" onclick={() => api.cancelFind()}><Icon name="x" /> Cancel</button>
      </div>
      <div class="progress-track" role="progressbar" aria-label="Finding duplicates">
        {#if progress.phase === "hashing" && progress.total > 0}
          <span class="progress-fill" style:width={`${percent(progress.done, progress.total)}%`}></span>
        {:else}
          <span class="progress-fill indeterminate"></span>
        {/if}
      </div>
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
      · compared the full contents of {plural(result.filesHashed, "file")} ({formatSize(result.bytesHashed)})
    </p>
    {#if groups.length === 0}
      <div class="empty muted">No duplicate files found.</div>
    {:else}
      <div class="bulk">
        <strong>{plural(groups.length, "group")} · {formatSize(groups.reduce((s, g) => s + g.wasted, 0))} in extra copies</strong>
        <span class="muted small">Nothing is picked for you. One copy of each file always stays.</span>
        <span class="spacer"></span>
        <button class="btn" onclick={() => (picks = pickAllButOne(groups, "newest"))}>Pick extras, keep newest</button>
        <button class="btn" onclick={() => (picks = pickAllButOne(groups, "oldest"))}>Pick extras, keep oldest</button>
        <button class="btn ghost" onclick={() => (picks = new Map())} disabled={totals.count === 0}>Clear</button>
      </div>
      <ul class="groups">
        {#each groups as g (g.index)}
          {@const gp = picks.get(g.index)}
          <li class="card group">
            <div class="group-head">
              <strong>{g.files.length} copies of {formatSize(g.size)}</strong>
              <span class="muted">{formatSize(g.size * (g.files.length - 1))} extra</span>
              <span class="muted mono small" title={`BLAKE3 ${g.hash}`}>{g.hash.slice(0, 12)}</span>
            </div>
            <ul class="copies">
              {#each g.files as f (f.index)}
                {@const p = splitPath(f.path)}
                {@const on = gp?.has(f.index) ?? false}
                {@const allowed = canPick(g, picks, f.index)}
                <li class:picked={on}>
                  <input
                    type="checkbox"
                    checked={on}
                    disabled={!allowed}
                    title={allowed ? "Move this copy to the recovery bin" : "Keep at least one copy"}
                    aria-label={`Move ${f.path} to the recovery bin`}
                    onchange={(e) => toggle(g, f.index, e.currentTarget.checked)}
                  />
                  <span class="file">
                    <span class="ellipsis" title={f.path}>{p.name}</span>
                    <span class="muted ellipsis small" title={p.folder}>{p.folder}</span>
                  </span>
                  <span class="muted small" title={formatDateTime(f.modified)}>{formatDate(f.modified)}</span>
                  <span class="state small">{on ? "Remove" : (gp?.size ?? 0) > 0 ? "Keep" : ""}</span>
                </li>
              {/each}
            </ul>
          </li>
        {/each}
      </ul>
    {/if}
    {#if result.unreadable > 0 || result.hidden > 0}
      <p class="muted small">
        {#if result.unreadable > 0}{plural(result.unreadable, "file")} couldn't be read and {result.unreadable === 1 ? "was" : "were"} skipped. {/if}
        {#if result.hidden > 0}{plural(result.hidden, "file")} in protected or excluded locations left out.{/if}
      </p>
    {/if}
    <footer class="bar">
      <span class="muted">{totals.count > 0 ? `Picked: ${sizeAndCount(totals.size, totals.count)}` : "Nothing picked"}</span>
      <button class="btn primary" onclick={review} disabled={totals.count === 0 || busy}>
        Move {sizeAndCount(totals.size, totals.count)} to the recovery bin…
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
  .bulk {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }
  .spacer {
    flex: 1;
  }
  .groups {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .group {
    padding: 8px 12px;
  }
  .group-head {
    display: flex;
    gap: 12px;
    align-items: baseline;
  }
  .copies {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
  }
  .copies li {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) 110px 60px;
    gap: 10px;
    align-items: center;
    padding: 3px 4px;
    border-radius: 6px;
  }
  .copies li.picked {
    background: var(--accent-2);
  }
  .file {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .state {
    text-align: right;
    color: var(--text-2);
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
