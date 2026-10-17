<script lang="ts">
  import { formatPercent, formatSize, percent } from "../lib/format.ts";
  import type { DriveInfo } from "../lib/types.ts";
  import Icon from "./Icon.svelte";

  let {
    drives,
    activeRoot,
    disabled,
    onScan,
  }: {
    drives: DriveInfo[];
    activeRoot: string | null;
    disabled: boolean;
    onScan: (root: string) => void;
  } = $props();
</script>

<div class="drives" role="group" aria-label="Drives">
  {#each drives as d (d.root)}
    {@const used = Math.max(0, d.total - d.free)}
    <button
      class="drive"
      class:active={activeRoot === d.root}
      {disabled}
      onclick={() => onScan(d.root)}
      title={`Scan ${d.root}`}
    >
      <span class="icon"><Icon name="drive" size={20} /></span>
      <span class="text">
        <span class="name">
          {d.root}
          {#if d.label}<span class="muted">{d.label}</span>{/if}
          {#if d.isSystem}<span class="tag">System</span>{/if}
        </span>
        <span class="bar-track"><span class="bar-fill" style:width={`${percent(used, d.total)}%`}></span></span>
        <span class="muted small">
          {formatSize(d.free)} free of {formatSize(d.total)} · {formatPercent(d.free, d.total)} free
        </span>
      </span>
    </button>
  {/each}
</div>

<style>
  .drives {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .drive {
    display: flex;
    gap: 10px;
    align-items: center;
    min-width: 220px;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
    text-align: left;
  }
  .drive:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .drive.active {
    border-color: var(--accent);
    background: var(--accent-2);
  }
  .icon {
    color: var(--text-2);
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
  }
  .name {
    font-weight: 600;
    display: flex;
    gap: 6px;
    align-items: baseline;
  }
  .name .muted {
    font-weight: 400;
  }
  .tag {
    font-size: 11px;
    font-weight: 600;
    padding: 0 6px;
    border-radius: 10px;
    background: var(--surface-3);
    color: var(--text-2);
  }
  .small {
    font-size: 12px;
  }
</style>
