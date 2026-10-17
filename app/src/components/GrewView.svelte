<script lang="ts">
  import { type Backend, backend, errorMessage } from "../lib/backend.ts";
  import { signedSize, splitPath } from "../lib/files.ts";
  import { formatDate, formatDateTime, formatSize, percent } from "../lib/format.ts";
  import type { ChangeView, ComparisonView, SnapshotView } from "../lib/types.ts";
  import Icon from "./Icon.svelte";

  let {
    active,
    onShowInDisk,
    onOpenRule,
  }: {
    active: boolean;
    onShowInDisk: (path: string, root: string) => void;
    onOpenRule: (ruleId: string) => void;
  } = $props();

  let api = $state<Backend | null>(null);
  let snapshots = $state<SnapshotView[]>([]);
  let root = $state<string | null>(null);
  let newer = $state<string | null>(null);
  let older = $state<string | null>(null);
  let result = $state<ComparisonView | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  let roots = $derived([...new Set(snapshots.map((s) => s.root))]);
  let ofRoot = $derived(snapshots.filter((s) => s.root === root));
  let maxDelta = $derived(
    Math.max(1, ...(result?.grew ?? []).map((c) => c.delta), ...(result?.shrank ?? []).map((c) => -c.delta)),
  );

  // Refresh the list whenever the tab is shown (each scan adds a snapshot).
  $effect(() => {
    if (active) void load();
  });

  async function load() {
    api ??= await backend();
    try {
      snapshots = await api.listSnapshots();
      if (!root || !snapshots.some((s) => s.root === root)) root = snapshots[0]?.root ?? null;
      pickDefaults();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  /** Newest snapshot vs the one before it. */
  function pickDefaults() {
    const list = snapshots.filter((s) => s.root === root);
    newer = list[0]?.key ?? null;
    older = list[1]?.key ?? null;
    void compare();
  }

  async function compare() {
    result = null;
    if (!api || !newer || !older || newer === older) return;
    loading = true;
    error = null;
    try {
      result = await api.compareSnapshots(older, newer);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  function note(c: ChangeView): string {
    if (c.oldSize === null) return `new, or under ${formatSize(result?.threshold ?? 0)} before`;
    if (c.newSize === null) return `gone, or now under ${formatSize(result?.threshold ?? 0)}`;
    return `${formatSize(c.oldSize)} → ${formatSize(c.newSize)}`;
  }
</script>

<section class="view">
  <header class="view-head">
    <div>
      <h1>What grew</h1>
      <p class="muted">
        Every scan saves a small record of folder sizes. Compare two of them to see where space went. Folders under
        50 MB aren't recorded.
      </p>
    </div>
    <button class="btn" onclick={load}><Icon name="refresh" /> Refresh</button>
  </header>

  {#if error}
    <div class="alert" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button class="btn ghost" aria-label="Dismiss" onclick={() => (error = null)}><Icon name="x" /></button>
    </div>
  {/if}

  {#if snapshots.length === 0}
    <div class="empty muted">No snapshots yet. Scan a drive in the Disk tab, then come back after a later scan.</div>
  {:else}
    <div class="card pickers">
      <label>
        Drive or folder
        <select bind:value={root} onchange={pickDefaults}>
          {#each roots as r (r)}<option value={r}>{r}</option>{/each}
        </select>
      </label>
      <label>
        Compare
        <select bind:value={newer} onchange={compare}>
          {#each ofRoot as s (s.key)}<option value={s.key}>{formatDateTime(s.takenAt)}</option>{/each}
        </select>
      </label>
      <label>
        with
        <select bind:value={older} onchange={compare}>
          {#each ofRoot as s (s.key)}<option value={s.key}>{formatDateTime(s.takenAt)}</option>{/each}
        </select>
      </label>
    </div>

    {#if ofRoot.length < 2}
      <div class="empty muted">Only one snapshot of {root} so far. Scan it again later to see what changed.</div>
    {:else if loading}
      <p class="muted">Comparing…</p>
    {:else if result}
      <section class="card summary">
        <span class="big">{signedSize(result.totalDelta)}</span>
        <span>
          {result.root} {result.totalDelta >= 0 ? "grew" : "shrank"} between {formatDate(result.oldTakenAt)} and
          {formatDate(result.newTakenAt)}.
        </span>
      </section>

      {#each [{ title: "Grew the most", list: result.grew, icon: "up" as const }, { title: "Shrank the most", list: result.shrank, icon: "chevronDown" as const }] as section (section.title)}
        {#if section.list.length > 0}
          <section class="card">
            <h2><Icon name={section.icon} /> {section.title}</h2>
            <ul class="changes">
              {#each section.list as c (c.path)}
                {@const p = splitPath(c.path)}
                <li>
                  <div class="what">
                    <strong class="ellipsis" title={c.path}>{p.name || c.path}</strong>
                    <span class="muted small ellipsis" title={p.folder}>{p.folder}</span>
                    <span class="muted small">{note(c)}</span>
                  </div>
                  <div class="delta">
                    <span class="num">{signedSize(c.delta)}</span>
                    <span class="bar-track"><span class="bar-fill" style:width={`${percent(Math.abs(c.delta), maxDelta)}%`}></span></span>
                  </div>
                  <div class="actions">
                    <button class="btn" onclick={() => onShowInDisk(c.path, result!.root)}><Icon name="drive" /> Show in Disk</button>
                    {#if c.rule}
                      {@const rule = c.rule}
                      <button class="btn" onclick={() => onOpenRule(rule.id)} title="Open this item in the Cleaner">
                        <Icon name="sparkle" /> Clean: {rule.name}
                      </button>
                    {/if}
                  </div>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      {/each}
      {#if result.grew.length === 0 && result.shrank.length === 0}
        <div class="empty muted">No folder changed by more than 10 MB.</div>
      {/if}
    {/if}
  {/if}
</section>

<style>
  .empty {
    padding: 30px;
    text-align: center;
  }
  .pickers {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
    align-items: center;
  }
  select {
    font: inherit;
    color: inherit;
    padding: 4px 6px;
    margin-left: 6px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
  }
  .summary {
    display: flex;
    gap: 14px;
    align-items: baseline;
  }
  .big {
    font-size: 24px;
    font-weight: 600;
  }
  h2 {
    display: flex;
    gap: 6px;
    align-items: center;
    margin: 0 0 6px;
    font-size: 15px;
  }
  .changes {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .changes li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 200px auto;
    gap: 14px;
    align-items: center;
    padding: 8px 0;
    border-top: 1px solid var(--surface-2);
  }
  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .small {
    font-size: 12px;
  }
  .delta {
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-items: flex-end;
  }
  .delta .bar-track {
    width: 100%;
  }
  .actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
</style>
