<script lang="ts">
  import { plural } from "../lib/cleaner.ts";
  import { formatCount, formatSize } from "../lib/format.ts";
  import type { CleanReport } from "../lib/types.ts";
  import Icon from "./Icon.svelte";

  let {
    report,
    onRecover,
    onDismiss,
  }: {
    report: CleanReport;
    onRecover: () => void;
    onDismiss: () => void;
  } = $props();

  let moreSkipped = $derived(report.skippedCount - report.skipped.length);
</script>

<section class="result card" role="status" aria-label="Cleanup result">
  <div class="head">
    <h2><Icon name="check" size={18} /> Cleanup finished</h2>
    <button class="btn ghost" aria-label="Dismiss" onclick={onDismiss}><Icon name="x" /></button>
  </div>
  <p class="freed">
    Disk space freed: <strong>{formatSize(report.freedBytes)}</strong>
  </p>
  <ul>
    {#if report.deletedCount > 0}
      <li>
        Deleted {plural(report.deletedCount, "file")} ({formatSize(report.deletedBytes)}){#if report.wipedCount > 0}, {report.wipedCount.toLocaleString()} of them overwritten first (secure delete){/if}.
      </li>
    {/if}
    {#if report.binnedCount > 0}
      <li>
        Moved {plural(report.binnedCount, "file")} ({formatSize(report.binnedBytes)}) to the recovery bin.
        <button class="link-btn" onclick={onRecover}>Open the Recover tab</button>
        to put them back. Their space is freed when the bin is emptied or the batch expires.
      </li>
    {/if}
    {#if report.foldersRemoved > 0}
      <li>Removed {plural(report.foldersRemoved, "empty folder")}.</li>
    {/if}
    {#if report.evicted.batches > 0}
      <li>
        Permanently deleted {plural(report.evicted.batches, "old recovery batch", "old recovery batches")}
        ({formatSize(report.evicted.bytes)}) to stay within the bin's limits.
      </li>
    {/if}
  </ul>
  {#if report.skippedCount > 0}
    <details>
      <summary>
        <Icon name="alert" size={14} />
        {plural(report.skippedCount, "file was", "files were")} skipped
      </summary>
      <ul class="skipped">
        {#each report.skipped as s (s.path)}
          <li><span class="mono ellipsis" title={s.path}>{s.path}</span> <span class="muted">{s.reason}</span></li>
        {/each}
        {#if moreSkipped > 0}
          <li class="muted">…and {formatCount(moreSkipped)} more. Every file is listed in the cleanup log.</li>
        {/if}
      </ul>
    </details>
  {/if}
</section>

<style>
  .result {
    border-color: var(--border);
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  h2 {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 16px;
    color: var(--text);
  }
  .freed {
    margin: 4px 0;
    font-size: 15px;
  }
  ul {
    margin: 4px 0;
    padding-left: 20px;
  }
  summary {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    cursor: pointer;
    color: var(--text);
  }
  .skipped {
    max-height: 180px;
    overflow: auto;
    font-size: 12px;
  }
  .skipped li {
    display: flex;
    gap: 8px;
  }
  .skipped .mono {
    max-width: 70%;
  }
</style>
