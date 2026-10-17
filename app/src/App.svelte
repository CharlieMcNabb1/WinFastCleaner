<script lang="ts">
  import { onMount } from "svelte";

  import CleanerView from "./components/CleanerView.svelte";
  import DiskView from "./components/DiskView.svelte";
  import FilesView from "./components/FilesView.svelte";
  import GrewView from "./components/GrewView.svelte";
  import Icon, { type IconName } from "./components/Icon.svelte";
  import PrivacyView from "./components/PrivacyView.svelte";
  import RecoverView from "./components/RecoverView.svelte";
  import SettingsView from "./components/SettingsView.svelte";
  import { backend } from "./lib/backend.ts";
  import type { AppInfo } from "./lib/types.ts";

  type Tab = "disk" | "cleaner" | "privacy" | "files" | "grew" | "recover" | "settings";
  const TABS: { id: Tab; label: string; icon: IconName }[] = [
    { id: "disk", label: "Disk", icon: "drive" },
    { id: "cleaner", label: "Cleaner", icon: "sparkle" },
    { id: "privacy", label: "Privacy", icon: "shield" },
    { id: "files", label: "Files", icon: "file" },
    { id: "grew", label: "What grew", icon: "trend" },
    { id: "recover", label: "Recover", icon: "restore" },
    { id: "settings", label: "Settings", icon: "sliders" },
  ];

  let info = $state<AppInfo | null>(null);
  let tab = $state<Tab>("disk");
  // Tabs stay alive once opened, so long operations keep running while the
  // user browses elsewhere.
  let opened = $state<Set<Tab>>(new Set(["disk"]));

  function go(t: Tab) {
    tab = t;
    if (!opened.has(t)) opened = new Set([...opened, t]);
  }

  // Cross-tab links from "What grew".
  let seq = 0;
  let diskTarget = $state<{ path: string; root: string; seq: number } | null>(null);
  let cleanerFocus = $state<{ ruleId: string; seq: number } | null>(null);

  function showInDisk(path: string, root: string) {
    diskTarget = { path, root, seq: ++seq };
    go("disk");
  }

  function openRule(ruleId: string) {
    cleanerFocus = { ruleId, seq: ++seq };
    go("cleaner");
  }

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
      {#each TABS as t (t.id)}
        <li>
          <button class="tab" class:active={tab === t.id} aria-current={tab === t.id ? "page" : undefined} onclick={() => go(t.id)}>
            <Icon name={t.icon} /> {t.label}
          </button>
        </li>
      {/each}
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
    <div class="pane" hidden={tab !== "disk"}><DiskView {info} target={diskTarget} onNavigate={go} /></div>
    {#if opened.has("cleaner")}
      <div class="pane" hidden={tab !== "cleaner"}>
        <CleanerView {info} active={tab === "cleaner"} focus={cleanerFocus} onNavigate={go} />
      </div>
    {/if}
    {#if opened.has("privacy")}
      <div class="pane" hidden={tab !== "privacy"}><PrivacyView active={tab === "privacy"} /></div>
    {/if}
    {#if opened.has("files")}
      <div class="pane" hidden={tab !== "files"}><FilesView active={tab === "files"} onNavigate={go} /></div>
    {/if}
    {#if opened.has("grew")}
      <div class="pane" hidden={tab !== "grew"}>
        <GrewView active={tab === "grew"} onShowInDisk={showInDisk} onOpenRule={openRule} />
      </div>
    {/if}
    {#if opened.has("recover")}
      <div class="pane" hidden={tab !== "recover"}><RecoverView active={tab === "recover"} /></div>
    {/if}
    {#if opened.has("settings")}
      <div class="pane" hidden={tab !== "settings"}><SettingsView {info} active={tab === "settings"} /></div>
    {/if}
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
  .tab:hover:not(.active) {
    background: var(--surface-2);
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
  .pane {
    height: 100%;
  }
  .pane[hidden] {
    display: none;
  }
</style>
