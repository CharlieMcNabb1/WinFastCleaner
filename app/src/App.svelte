<script lang="ts">
  import { onMount } from "svelte";

  import DiskView from "./components/DiskView.svelte";
  import Icon from "./components/Icon.svelte";
  import { backend } from "./lib/backend.ts";
  import type { AppInfo } from "./lib/types.ts";

  let info = $state<AppInfo | null>(null);

  onMount(async () => {
    try {
      info = await (await backend()).appInfo();
    } catch {
      info = null;
    }
  });
</script>

<div class="shell">
  <nav class="sidebar" aria-label="Sections">
    <div class="brand">
      <img src="/favicon.png" alt="" width="24" height="24" />
      <span>WinFastCleaner</span>
    </div>
    <ul>
      <li>
        <button class="tab active" aria-current="page"><Icon name="drive" /> Disk</button>
      </li>
    </ul>
    <div class="foot muted">
      <p>No ads · No telemetry · Works offline</p>
      {#if info}
        <p>
          v{info.version}
          {#if info.platform === "windows"}
            · {info.elevated ? "Administrator" : "Standard user"}
          {/if}
        </p>
      {/if}
    </div>
  </nav>
  <main>
    <DiskView {info} />
  </main>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 200px 1fr;
    height: 100%;
  }
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px 10px;
    border-right: 1px solid var(--border);
    background: var(--surface);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 6px 8px;
    font-weight: 600;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .tab {
    display: flex;
    gap: 8px;
    align-items: center;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    border-radius: 6px;
    background: none;
    text-align: left;
  }
  .tab.active {
    background: var(--accent-2);
    color: var(--accent);
    font-weight: 600;
  }
  .foot {
    margin-top: auto;
    font-size: 12px;
    padding: 0 6px;
  }
  .foot p {
    margin: 4px 0;
  }
  main {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
</style>
