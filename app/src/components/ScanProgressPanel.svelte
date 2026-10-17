<script lang="ts">
  import { formatCount, formatSize, percent } from "../lib/format.ts";
  import type { ScanProgress } from "../lib/types.ts";
  import Icon from "./Icon.svelte";

  let {
    path,
    progress,
    canBrowse,
    onCancel,
  }: {
    path: string | null;
    progress: ScanProgress | null;
    canBrowse: boolean;
    onCancel: () => void;
  } = $props();

  let determinate = $derived(progress?.phase === "readingMft" && progress.total > 0);
  let value = $derived(determinate && progress ? percent(progress.done, progress.total) : 0);
  let phaseText = $derived.by(() => {
    switch (progress?.phase) {
      case "readingMft":
        return "Reading the Master File Table (fast scan)";
      case "building":
        return "Building the folder tree";
      case "walking":
        return "Scanning folders";
      default:
        return "Starting";
    }
  });
</script>

<section class="progress" aria-live="polite" aria-label="Scan progress">
  <div class="head">
    <div>
      <strong>Scanning {path}</strong>
      <span class="muted">· {phaseText}</span>
    </div>
    <button class="btn" onclick={onCancel}><Icon name="x" /> Cancel</button>
  </div>
  <div
    class="track"
    role="progressbar"
    aria-valuemin={0}
    aria-valuemax={100}
    aria-valuenow={determinate ? Math.round(value) : undefined}
    aria-label="Scan progress"
  >
    {#if determinate}
      <span class="fill" style:width={`${value}%`}></span>
    {:else}
      <span class="fill indeterminate"></span>
    {/if}
  </div>
  <div class="stats muted">
    {#if progress}
      <span>{formatCount(progress.files)} files</span>
      <span>{formatCount(progress.dirs)} folders</span>
      <span>{formatSize(progress.bytes)}</span>
      {#if progress.current}<span class="current" title={progress.current}>{progress.current}</span>{/if}
    {/if}
  </div>
  {#if canBrowse}
    <p class="muted hint">You can keep browsing the previous results while this runs.</p>
  {/if}
</section>

<style>
  .progress {
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }
  .track {
    position: relative;
    height: 8px;
    border-radius: 4px;
    background: var(--bar-track);
    overflow: hidden;
  }
  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--bar);
    border-radius: 4px;
    transition: width 0.2s;
  }
  .indeterminate {
    width: 30%;
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      left: -30%;
    }
    to {
      left: 100%;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .indeterminate {
      animation: none;
      width: 100%;
      opacity: 0.4;
    }
  }
  .stats {
    display: flex;
    gap: 14px;
    font-size: 12px;
    min-width: 0;
  }
  .current {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    font-family: var(--mono);
  }
  .hint {
    margin: 0;
    font-size: 12px;
  }
</style>
