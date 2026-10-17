<script lang="ts">
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import { type Backend, backend, errorMessage } from "../lib/backend.ts";
  import { plural } from "../lib/cleaner.ts";
  import { formatDate } from "../lib/format.ts";
  import { canToggle, matchesFilter, SOURCE_LABEL, sortStartup } from "../lib/system.ts";
  import type { AppInfo, StartupItem, StartupView } from "../lib/types.ts";
  import Icon from "./Icon.svelte";

  let { info, active }: { info: AppInfo | null; active: boolean } = $props();

  let api = $state<Backend | null>(null);
  let view = $state<StartupView | null>(null);
  let loading = $state(false);
  let filter = $state("");
  const busy = new SvelteSet<string>();
  let error = $state<string | null>(null);

  let items = $derived(view ? sortStartup(view.items).filter((i) => matchesFilter(i, filter)) : []);
  let enabledCount = $derived(view?.items.filter((i) => i.enabled).length ?? 0);
  let elevated = $derived(view?.elevated ?? info?.elevated ?? false);
  let lockedMachine = $derived(!elevated && (view?.items.some((i) => i.machine) ?? false));

  $effect(() => {
    if (active) untrack(() => void load());
  });

  async function load() {
    api ??= await backend();
    loading = true;
    try {
      view = await api.startupList();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  async function toggle(item: StartupItem, enabled: boolean) {
    if (!api || !view) return;
    busy.add(item.id);
    error = null;
    try {
      const updated = await api.startupSetEnabled(item.id, enabled);
      view = { ...view, items: view.items.map((i) => (i.id === updated.id ? updated : i)) };
    } catch (e) {
      error = errorMessage(e);
      // Show the item as it really is.
      await load();
    } finally {
      busy.delete(item.id);
    }
  }

  async function restartAsAdmin() {
    try {
      await (api ?? (await backend())).relaunchElevated();
    } catch (e) {
      error = errorMessage(e);
    }
  }
</script>

<section class="view">
  <header class="view-head">
    <div>
      <h1>Startup</h1>
      <p class="muted">
        Apps that start when you sign in. Turning one off doesn't uninstall or delete anything, and you can turn it back on
        at any time (it's the same switch Task Manager uses).
      </p>
    </div>
    <button class="btn" onclick={load} disabled={loading}><Icon name="refresh" /> Refresh</button>
  </header>

  {#if lockedMachine}
    <div class="notice">
      <Icon name="shield" size={18} />
      <span>Items that start for every user need administrator rights to change.</span>
      <button class="btn" onclick={restartAsAdmin}><Icon name="shield" /> Restart as administrator</button>
    </div>
  {/if}

  {#if error}
    <div class="alert" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button class="btn ghost" aria-label="Dismiss" onclick={() => (error = null)}><Icon name="x" /></button>
    </div>
  {/if}

  {#if view}
    {#each view.errors as e, i (i)}
      <p class="muted small"><Icon name="alert" size={12} /> {e}</p>
    {/each}

    <section class="card" aria-labelledby="startup-apps">
      <div class="card-head">
        <h2 id="startup-apps">Apps</h2>
        <span class="muted">{enabledCount} of {plural(view.items.length, "item")} turned on</span>
        <input class="filter" type="search" placeholder="Filter" aria-label="Filter startup apps" bind:value={filter} />
      </div>
      {#if items.length === 0}
        <p class="muted">{view.items.length === 0 ? "Nothing starts when you sign in." : "Nothing matches the filter."}</p>
      {:else}
        <ul class="items">
          {#each items as item (item.id)}
            {@const allowed = canToggle(item, elevated)}
            <li class:off={!item.enabled}>
              <label class="switch" title={allowed ? "" : "Needs administrator rights"}>
                <input
                  type="checkbox"
                  role="switch"
                  checked={item.enabled}
                  disabled={!allowed || busy.has(item.id)}
                  aria-label={`Start ${item.name} when you sign in`}
                  onchange={(e) => toggle(item, e.currentTarget.checked)}
                />
                <span class="state">{item.enabled ? "On" : "Off"}</span>
              </label>
              <div class="what">
                <strong>
                  {item.name}
                  {#if item.machine}<span class="tag" title="Starts for every user"><Icon name="lock" size={11} /> All users</span>{/if}
                </strong>
                <span class="muted small">
                  {item.publisher ?? "Unknown publisher"} · {SOURCE_LABEL[item.source]}
                  {#if !item.enabled && item.disabledAt}· turned off {formatDate(item.disabledAt)}{/if}
                </span>
                <span class="mono muted ellipsis" title={item.command}>{item.command || "(no command)"}</span>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <details class="card services">
      <summary>
        <h2>Services set to start automatically</h2>
        <span class="muted">({view.services.length}, read-only)</span>
      </summary>
      <p class="muted small">
        Background services from other publishers. Windows' own services aren't listed. These are shown for information;
        to change one, use Windows' Services app.
      </p>
      {#if view.servicesError}
        <p class="muted">{view.servicesError}</p>
      {:else if view.services.length === 0}
        <p class="muted">None.</p>
      {:else}
        <table>
          <thead>
            <tr><th>Service</th><th>Publisher</th><th>Start</th><th>Now</th></tr>
          </thead>
          <tbody>
            {#each view.services as s (s.name)}
              <tr>
                <td>
                  <strong>{s.displayName}</strong>
                  {#if s.program}<br /><span class="mono muted ellipsis" title={s.program}>{s.program}</span>{/if}
                </td>
                <td>{s.publisher ?? "Unknown"}</td>
                <td>{s.delayed ? "Automatic (delayed)" : "Automatic"}</td>
                <td>{s.running ? "Running" : "Stopped"}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </details>
  {:else if loading}
    <p class="muted">Reading the startup list…</p>
  {/if}
</section>

<style>
  h2 {
    margin: 0;
    font-size: 15px;
  }
  .small {
    font-size: 12px;
  }
  .card-head {
    display: flex;
    gap: 12px;
    align-items: center;
    margin-bottom: 6px;
  }
  .filter {
    margin-left: auto;
    width: 200px;
  }
  .items {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .items li {
    display: grid;
    grid-template-columns: 70px minmax(0, 1fr);
    gap: 12px;
    align-items: center;
    padding: 8px 0;
    border-top: 1px solid var(--surface-2);
  }
  .items li.off .what strong {
    color: var(--text-2);
  }
  .switch {
    display: inline-flex;
    gap: 6px;
    align-items: center;
  }
  .state {
    font-size: 12px;
    font-weight: 600;
  }
  .what {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .tag {
    display: inline-flex;
    gap: 3px;
    align-items: center;
    margin-left: 6px;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-2);
  }
  .services summary {
    display: flex;
    gap: 8px;
    align-items: center;
    cursor: pointer;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
  }
  th,
  td {
    padding: 6px 8px 6px 0;
    border-top: 1px solid var(--surface-2);
    text-align: left;
    vertical-align: top;
  }
  th {
    font-size: 12px;
    color: var(--text-2);
    font-weight: 600;
  }
  th:first-child {
    width: 45%;
  }
  td .ellipsis {
    display: inline-block;
    max-width: 100%;
  }
</style>
