<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";

  import { type Backend, backend, errorMessage } from "../lib/backend.ts";
  import { plural, sizeAndCount } from "../lib/cleaner.ts";
  import { formatDateTime, formatSize, percent } from "../lib/format.ts";
  import type { Batch, Conflict, RecoveryOverview } from "../lib/types.ts";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import TierBadge from "./TierBadge.svelte";

  let { active }: { active: boolean } = $props();

  let api = $state<Backend | null>(null);
  let overview = $state<RecoveryOverview | null>(null);
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);
  let working = $state(false);
  let openBatch = $state<Batch | null>(null);
  const picked = new SvelteSet<number>();

  /** A restore waiting for the user to choose what to do about conflicts. */
  let conflict = $state<{ batchId: string; itemIds: number[] | null; paths: string[] } | null>(null);
  let confirmDelete = $state<{ id: string; label: string; size: number; items: number } | null>(null);
  let confirmEmpty = $state(false);
  /** Secure delete is offered (Settings) and chosen in the open dialog. */
  let secureOffered = $state(false);
  let secure = $state(false);

  let totalSize = $derived(overview?.batches.reduce((s, b) => s + b.size, 0) ?? 0);
  let totalItems = $derived(overview?.batches.reduce((s, b) => s + b.items, 0) ?? 0);

  // Refresh whenever the tab is shown (a clean may have added a batch).
  $effect(() => {
    if (active) void refresh();
  });

  async function refresh() {
    api ??= await backend();
    try {
      const [o, settings] = await Promise.all([api.recoveryOverview(), api.getSettings()]);
      overview = o;
      secureOffered = settings.settings.secureDelete;
      if (openBatch && !overview.batches.some((b) => b.id === openBatch!.id)) openBatch = null;
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function toggleBatch(id: string) {
    if (!api) return;
    if (openBatch?.id === id) {
      openBatch = null;
      return;
    }
    try {
      openBatch = await api.recoveryBatch(id);
      picked.clear();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function restore(batchId: string, itemIds: number[] | null) {
    if (!api) return;
    error = null;
    message = null;
    try {
      const paths = await api.recoveryConflicts(batchId, itemIds);
      if (paths.length > 0) {
        conflict = { batchId, itemIds, paths };
        return;
      }
      await doRestore(batchId, itemIds, "skip");
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function doRestore(batchId: string, itemIds: number[] | null, mode: Conflict) {
    if (!api) return;
    conflict = null;
    working = true;
    try {
      const r = await api.recoveryRestore(batchId, itemIds, mode);
      const parts = [`Restored ${sizeAndCount(r.restoredBytes, r.restored)} to where they were.`];
      if (r.renamed.length > 0) parts.push(`${plural(r.renamed.length, "file was", "files were")} restored under a new name.`);
      if (r.skipped.length > 0) {
        parts.push(
          `${plural(r.skipped.length, "file was", "files were")} not restored: ` +
            r.skipped
              .slice(0, 3)
              .map(([p, why]) => `${p} (${why})`)
              .join("; ") +
            (r.skipped.length > 3 ? "; …" : ""),
        );
      }
      message = parts.join(" ");
      picked.clear();
      if (openBatch?.id === batchId) openBatch = await api.recoveryBatch(batchId).catch(() => null);
      await refresh();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      working = false;
    }
  }

  async function deleteBatch(id: string) {
    if (!api) return;
    confirmDelete = null;
    working = true;
    try {
      const r = await api.recoveryDelete(id, secure);
      message = `Permanently deleted ${sizeAndCount(r.bytes, r.files)}.` + (r.failed > 0 ? ` ${plural(r.failed, "file")} could not be deleted.` : "");
      await refresh();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      working = false;
    }
  }

  async function emptyBin() {
    if (!api) return;
    confirmEmpty = false;
    working = true;
    try {
      const r = await api.recoveryEmpty(secure);
      message = `Emptied the recovery bin: ${sizeAndCount(r.bytes, r.files)} permanently deleted.` + (r.failed > 0 ? ` ${plural(r.failed, "file")} could not be deleted.` : "");
      openBatch = null;
      await refresh();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      working = false;
    }
  }

  function togglePick(id: number, on: boolean) {
    if (on) picked.add(id);
    else picked.delete(id);
  }
</script>

<section class="view">
  <header class="view-head">
    <div>
      <h1>Recover</h1>
      <p class="muted">
        Files a cleanup moved to the recovery bin can be put back where they were. The oldest batches are deleted for good
        when the bin goes over the limits set in Settings.
      </p>
    </div>
    <div class="head-actions">
      <button class="btn" onclick={refresh} disabled={working}><Icon name="refresh" /> Refresh</button>
      <button
        class="btn danger"
        onclick={() => {
          secure = false;
          confirmEmpty = true;
        }}
        disabled={working || totalItems === 0}
      >
        <Icon name="trash" /> Empty bin
      </button>
    </div>
  </header>

  {#if error}
    <div class="alert" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button class="btn ghost" aria-label="Dismiss" onclick={() => (error = null)}><Icon name="x" /></button>
    </div>
  {/if}
  {#if message}
    <div class="notice" role="status">
      <Icon name="check" size={18} />
      <span>{message}</span>
      <button class="btn ghost" aria-label="Dismiss" onclick={() => (message = null)}><Icon name="x" /></button>
    </div>
  {/if}

  {#if overview}
    <section class="card usage" aria-label="Recovery bin usage">
      <strong>In the bin: {sizeAndCount(totalSize, totalItems)}</strong>
      <span class="muted">
        Limits: {formatSize(overview.limits.maxBytes)} or {overview.limits.maxPercent}% of each drive (whichever is smaller),
        {plural(overview.limits.maxBatches, "batch", "batches")}, {plural(overview.limits.maxAgeDays, "day")}.
      </span>
      {#each overview.usage as u (u.drive)}
        <div class="drive">
          <span class="mono">{u.drive}</span>
          <div class="progress-track" role="img" aria-label={`${formatSize(u.bytes)} of ${formatSize(u.limit)} used on ${u.drive}`}>
            <span class="progress-fill" style:width={`${Math.min(100, percent(u.bytes, u.limit))}%`}></span>
          </div>
          <span class="muted num">{formatSize(u.bytes)} of {formatSize(u.limit)}</span>
        </div>
      {/each}
    </section>

    {#if overview.batches.length === 0}
      <div class="empty muted">The recovery bin is empty.</div>
    {/if}

    <ul class="batches">
      {#each overview.batches as b (b.id)}
        {@const isOpen = openBatch?.id === b.id}
        <li class="card batch">
          <div class="batch-row">
            <button class="btn ghost" onclick={() => toggleBatch(b.id)} aria-expanded={isOpen} aria-label={`Show files from ${formatDateTime(b.createdAt)}`}>
              <Icon name={isOpen ? "chevronDown" : "chevronRight"} />
            </button>
            <div class="batch-title">
              <strong>{formatDateTime(b.createdAt)}</strong>
              <span class="muted">{b.label} · {sizeAndCount(b.size, b.items)}</span>
            </div>
            <button class="btn" onclick={() => restore(b.id, null)} disabled={working}><Icon name="restore" /> Restore all</button>
            <button
              class="btn danger"
              onclick={() => {
                secure = false;
                confirmDelete = { id: b.id, label: formatDateTime(b.createdAt), size: b.size, items: b.items };
              }}
              disabled={working}
            >
              <Icon name="trash" /> Delete
            </button>
          </div>
          {#if isOpen && openBatch}
            <div class="items">
              <ul>
                {#each openBatch.items as item (item.id)}
                  <li>
                    <input
                      type="checkbox"
                      checked={picked.has(item.id)}
                      aria-label={`Select ${item.original}`}
                      onchange={(e) => togglePick(item.id, e.currentTarget.checked)}
                    />
                    <TierBadge tier={item.tier} />
                    <span class="mono ellipsis" title={item.original}>{item.original}</span>
                    <span class="num muted">{formatSize(item.size)}</span>
                  </li>
                {/each}
              </ul>
              <button class="btn" onclick={() => restore(b.id, [...picked])} disabled={picked.size === 0 || working}>
                <Icon name="restore" /> Restore {picked.size > 0 ? plural(picked.size, "selected file") : "selected"}
              </button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<Modal open={conflict !== null} title="Some files already exist" onClose={() => (conflict = null)}>
  {#if conflict}
    <p>
      {plural(conflict.paths.length, "file", "files")} from this batch {conflict.paths.length === 1 ? "has" : "have"} a file with the
      same name at {conflict.paths.length === 1 ? "its" : "their"} original location:
    </p>
    <ul class="conflicts">
      {#each conflict.paths.slice(0, 8) as p (p)}<li class="mono ellipsis" title={p}>{p}</li>{/each}
      {#if conflict.paths.length > 8}<li class="muted">…and {conflict.paths.length - 8} more</li>{/if}
    </ul>
    <p class="muted">Files without a conflict are restored either way.</p>
  {/if}
  {#snippet actions()}
    {#if conflict}
      {@const c = conflict}
      <button class="btn" onclick={() => doRestore(c.batchId, c.itemIds, "skip")}>Skip those</button>
      <button class="btn" onclick={() => doRestore(c.batchId, c.itemIds, "keepBoth")}>Keep both</button>
      <button class="btn primary" onclick={() => doRestore(c.batchId, c.itemIds, "overwrite")} title="The existing files go to the recovery bin first">
        Overwrite
      </button>
    {/if}
  {/snippet}
</Modal>

<Modal open={confirmDelete !== null} title="Delete this batch for good?" onClose={() => (confirmDelete = null)}>
  {#if confirmDelete}
    <p>
      The batch from {confirmDelete.label} ({sizeAndCount(confirmDelete.size, confirmDelete.items)}) will be permanently deleted.
      It can't be restored afterwards.
    </p>
    {#if secureOffered}
      <label class="secure"><input type="checkbox" bind:checked={secure} /> Overwrite the files first (secure delete; slower, less effective on SSDs)</label>
    {/if}
  {/if}
  {#snippet actions()}
    <button class="btn" onclick={() => (confirmDelete = null)}>Cancel</button>
    {#if confirmDelete}
      {@const d = confirmDelete}
      <button class="btn danger primary" onclick={() => deleteBatch(d.id)}>
        <Icon name="trash" /> Delete {sizeAndCount(d.size, d.items)}
      </button>
    {/if}
  {/snippet}
</Modal>

<Modal open={confirmEmpty} title="Empty the recovery bin?" onClose={() => (confirmEmpty = false)}>
  <p>Everything in the bin ({sizeAndCount(totalSize, totalItems)}) will be permanently deleted. It can't be restored afterwards.</p>
  {#if secureOffered}
    <label class="secure"><input type="checkbox" bind:checked={secure} /> Overwrite the files first (secure delete; slower, less effective on SSDs)</label>
  {/if}
  {#snippet actions()}
    <button class="btn" onclick={() => (confirmEmpty = false)}>Cancel</button>
    <button class="btn danger primary" onclick={emptyBin}><Icon name="trash" /> Delete {sizeAndCount(totalSize, totalItems)}</button>
  {/snippet}
</Modal>

<style>
  .head-actions {
    display: flex;
    gap: 8px;
  }
  .usage {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .drive {
    display: grid;
    grid-template-columns: 40px minmax(120px, 1fr) auto;
    gap: 10px;
    align-items: center;
  }
  .empty {
    padding: 30px;
    text-align: center;
  }
  .batches {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .batch {
    padding: 8px 10px;
  }
  .batch-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .batch-title {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .items {
    margin: 8px 0 4px 36px;
  }
  .items ul {
    list-style: none;
    margin: 0 0 8px;
    padding: 0;
    max-height: 320px;
    overflow: auto;
  }
  .items li {
    display: grid;
    grid-template-columns: auto auto minmax(0, 1fr) 90px;
    gap: 8px;
    align-items: center;
    padding: 2px 0;
  }
  .secure {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .conflicts {
    margin: 0;
    padding-left: 18px;
    max-height: 160px;
    overflow: auto;
  }
</style>
