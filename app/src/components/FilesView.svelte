<script lang="ts">
  import { type Backend, backend, errorMessage } from "../lib/backend.ts";
  import type { DriveInfo } from "../lib/types.ts";
  import DupesPanel from "./DupesPanel.svelte";
  import Icon from "./Icon.svelte";
  import LargeOldPanel from "./LargeOldPanel.svelte";

  let { active, onNavigate }: { active: boolean; onNavigate: (tab: "recover") => void } = $props();

  let api = $state<Backend | null>(null);
  let drives = $state<DriveInfo[]>([]);
  let root = $state<string | null>(null);
  let sub = $state<"large" | "dupes">("large");
  let largeBusy = $state(false);
  let dupesBusy = $state(false);
  let error = $state<string | null>(null);

  let started = false;
  $effect(() => {
    if (active && !started) {
      started = true;
      void init();
    }
  });

  async function init() {
    api = await backend();
    try {
      drives = await api.listDrives();
      root ??= (drives.find((d) => d.isSystem) ?? drives[0])?.root ?? null;
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function chooseFolder() {
    if (!api) return;
    try {
      const folder = await api.pickFolder("Choose a folder to search");
      if (folder) root = folder;
    } catch (e) {
      error = errorMessage(e);
    }
  }

  let isDrive = $derived(drives.some((d) => d.root === root));
</script>

<section class="view">
  <header class="view-head">
    <div>
      <h1>Files</h1>
      <p class="muted">
        Find your own large, old, and duplicate files. Nothing is picked for you, and anything you pick goes to the
        recovery bin.
      </p>
    </div>
  </header>

  <div class="look-in">
    <label for="files-root">Look in</label>
    <select id="files-root" bind:value={root} disabled={largeBusy || dupesBusy}>
      {#each drives as d (d.root)}
        <option value={d.root}>{d.root} {d.label}</option>
      {/each}
      {#if root && !isDrive}<option value={root}>{root}</option>{/if}
    </select>
    <button class="btn" onclick={chooseFolder} disabled={largeBusy || dupesBusy}><Icon name="folderOpen" /> Folder…</button>
  </div>

  {#if error}
    <div class="alert" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button class="btn ghost" aria-label="Dismiss" onclick={() => (error = null)}><Icon name="x" /></button>
    </div>
  {/if}

  <div class="tabs" role="tablist" aria-label="Files">
    <button role="tab" aria-selected={sub === "large"} class:active={sub === "large"} onclick={() => (sub = "large")}>
      Large and old files
    </button>
    <button role="tab" aria-selected={sub === "dupes"} class:active={sub === "dupes"} onclick={() => (sub = "dupes")}>
      Duplicates
    </button>
  </div>

  {#if api}
    <div role="tabpanel" hidden={sub !== "large"}>
      <LargeOldPanel {api} {root} bind:busy={largeBusy} {onNavigate} />
    </div>
    <div role="tabpanel" hidden={sub !== "dupes"}>
      <DupesPanel {api} {root} bind:busy={dupesBusy} {onNavigate} />
    </div>
  {/if}
</section>

<style>
  .look-in {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  select {
    font: inherit;
    color: inherit;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    min-width: 200px;
  }
  .tabs {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    border: 0;
    border-bottom: 2px solid transparent;
    background: none;
    padding: 6px 12px;
    color: var(--text-2);
  }
  .tabs button.active {
    color: var(--accent);
    border-bottom-color: var(--accent);
    font-weight: 600;
  }
  [hidden] {
    display: none;
  }
</style>
