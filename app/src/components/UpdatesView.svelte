<script lang="ts">
  import { untrack } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";

  import { type Backend, backend, errorMessage } from "../lib/backend.ts";
  import { plural } from "../lib/cleaner.ts";
  import { formatDateTime } from "../lib/format.ts";
  import { updatable } from "../lib/system.ts";
  import type { UpdatesStatus, Upgrade, UpgradeResult } from "../lib/types.ts";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";

  let { active }: { active: boolean } = $props();

  let api = $state<Backend | null>(null);
  let status = $state<UpdatesStatus | null>(null);
  let list = $state<Upgrade[] | null>(null);
  let checkedAt = $state<number | null>(null);
  let checking = $state(false);
  // Nothing is picked for the user.
  const picked = new SvelteSet<string>();
  let confirming = $state(false);
  let running = $state<{ index: number; total: number; id: string | null; stopping: boolean } | null>(null);
  const results = new SvelteMap<string, UpgradeResult>();
  let error = $state<string | null>(null);

  let canPick = $derived(list ? updatable(list) : []);
  let chosen = $derived(list?.filter((u) => picked.has(u.id)) ?? []);
  let allPicked = $derived(canPick.length > 0 && canPick.every((u) => picked.has(u.id)));
  let doneSummary = $derived.by(() => {
    if (results.size === 0 || running) return null;
    const all = [...results.values()];
    return { ok: all.filter((r) => r.ok).length, failed: all.filter((r) => !r.ok).length };
  });

  // The first visit checks for updates.
  let started = false;
  $effect(() => {
    if (active && !started) {
      started = true;
      untrack(() => void init());
    }
  });

  async function init() {
    api ??= await backend();
    try {
      status = await api.updatesStatus();
      checkedAt = status.last?.checkedAt ?? null;
      if (status.installed) await check();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function check() {
    if (!api || checking || running) return;
    checking = true;
    error = null;
    try {
      list = await api.updatesList();
      checkedAt = Math.floor(Date.now() / 1000);
      for (const id of [...picked]) if (!list.some((u) => u.id === id && u.canUpdate)) picked.delete(id);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      checking = false;
    }
  }

  function pickAll(on: boolean) {
    picked.clear();
    if (on) for (const u of canPick) picked.add(u.id);
  }

  async function apply() {
    confirming = false;
    if (!api || chosen.length === 0) return;
    const ids = chosen.map((u) => u.id);
    results.clear();
    error = null;
    running = { index: 0, total: ids.length, id: null, stopping: false };
    try {
      await api.updatesApply(ids, (e) => {
        switch (e.type) {
          case "started":
            running = { index: e.index, total: e.total, id: e.id, stopping: running?.stopping ?? false };
            break;
          case "finished":
            results.set(e.result.id, e.result);
            break;
          case "done":
            running = null;
            picked.clear();
            void check();
            break;
        }
      });
    } catch (e) {
      running = null;
      error = errorMessage(e);
    }
  }

  async function stop() {
    if (!api || !running) return;
    running = { ...running, stopping: true };
    await api.updatesCancel();
  }

  function nameOf(id: string): string {
    return list?.find((u) => u.id === id)?.name ?? id;
  }
</script>

<section class="view">
  <header class="view-head">
    <div>
      <h1>Updates</h1>
      <p class="muted">
        Apps with newer versions, found by winget (the Windows Package Manager). Nothing is picked for you, and each update
        runs that app's own installer.
      </p>
    </div>
    {#if status?.installed}
      <button class="btn" onclick={check} disabled={checking || running !== null}><Icon name="refresh" /> Check again</button>
    {/if}
  </header>

  {#if error}
    <div class="alert" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button class="btn ghost" aria-label="Dismiss" onclick={() => (error = null)}><Icon name="x" /></button>
    </div>
  {/if}

  {#if status && !status.installed}
    <section class="card missing">
      <h2><Icon name="info" size={16} /> winget isn't installed</h2>
      <p>{status.missingMessage}</p>
    </section>
  {:else if status}
    {#if checking}
      <section class="card running" aria-live="polite">
        <div class="running-head">
          <strong>Checking for updates…</strong>
          <span class="muted">This can take a minute.</span>
        </div>
        <div class="progress-track" role="progressbar" aria-label="Checking for updates">
          <span class="progress-fill indeterminate"></span>
        </div>
      </section>
    {/if}

    {#if running}
      <section class="card running" aria-live="polite">
        <div class="running-head">
          <strong>
            Updating {running.index + 1} of {running.total}{running.id ? `: ${nameOf(running.id)}` : ""}…
          </strong>
          <button class="btn" onclick={stop} disabled={running.stopping}>
            <Icon name="x" />
            {running.stopping ? "Stopping after this app…" : "Cancel"}
          </button>
        </div>
        <div
          class="progress-track"
          role="progressbar"
          aria-label="Update progress"
          aria-valuemin={0}
          aria-valuemax={running.total}
          aria-valuenow={results.size}
        >
          <span class="progress-fill" style:width={`${(results.size * 100) / Math.max(1, running.total)}%`}></span>
        </div>
        <p class="muted small">An app's installer may ask for permission. Cancel stops after the current app finishes.</p>
      </section>
    {/if}

    {#if doneSummary}
      <section class="card result" role="status">
        <strong>
          {doneSummary.ok > 0 ? `${plural(doneSummary.ok, "app")} updated.` : ""}
          {doneSummary.failed > 0 ? `${plural(doneSummary.failed, "update")} failed.` : ""}
        </strong>
        <button class="btn ghost" aria-label="Dismiss" onclick={() => results.clear()}><Icon name="x" /></button>
      </section>
    {/if}

    {#if list}
      <section class="card" aria-labelledby="updates-list">
        <div class="card-head">
          <h2 id="updates-list">
            {list.length === 0 ? "Your apps are up to date" : plural(list.length, "update") + " available"}
          </h2>
          {#if checkedAt}<span class="muted small">Checked {formatDateTime(checkedAt)}</span>{/if}
        </div>
        {#if list.length > 0}
          <table>
            <thead>
              <tr>
                <th class="pick">
                  <input
                    type="checkbox"
                    aria-label="Pick all"
                    checked={allPicked}
                    disabled={canPick.length === 0 || running !== null}
                    onchange={(e) => pickAll(e.currentTarget.checked)}
                  />
                </th>
                <th>App</th>
                <th>Installed</th>
                <th>Available</th>
                <th>Result</th>
              </tr>
            </thead>
            <tbody>
              {#each list as u (u.id)}
                {@const r = results.get(u.id)}
                <tr>
                  <td class="pick">
                    <input
                      type="checkbox"
                      aria-label={`Update ${u.name}`}
                      checked={picked.has(u.id)}
                      disabled={!u.canUpdate || running !== null}
                      onchange={(e) => (e.currentTarget.checked ? picked.add(u.id) : picked.delete(u.id))}
                    />
                  </td>
                  <td>
                    <strong>{u.name}</strong><br />
                    <span class="mono muted">{u.id}</span>
                    {#if !u.canUpdate}<br /><span class="muted small">Update this one from the app itself.</span>{/if}
                  </td>
                  <td class="mono">{u.version}</td>
                  <td class="mono">{u.available}</td>
                  <td>
                    {#if r}
                      <span class={r.ok ? "ok" : "fail"}>
                        <Icon name={r.ok ? "check" : "alert"} size={12} />
                        {r.ok ? "Done" : "Failed"}
                      </span>
                      <span class="muted small">{r.message}</span>
                    {:else if running?.id === u.id}
                      <span class="muted">Updating…</span>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </section>

      <footer class="bar">
        <span class="muted">{chosen.length > 0 ? `${plural(chosen.length, "app")} picked` : "Nothing picked"}</span>
        <button class="btn primary" onclick={() => (confirming = true)} disabled={chosen.length === 0 || running !== null || checking}>
          <Icon name="download" /> Update selected
        </button>
      </footer>
    {/if}
  {/if}
</section>

<Modal open={confirming} title={`Update ${plural(chosen.length, "app")}?`} onClose={() => (confirming = false)}>
  <ul class="confirm">
    {#each chosen as u (u.id)}
      <li><strong>{u.name}</strong> {u.version} → {u.available}</li>
    {/each}
  </ul>
  <p class="muted">
    winget downloads each update from its publisher and runs the installer quietly. An installer may ask for permission,
    and an app that is open may need to be closed first.
  </p>
  {#snippet actions()}
    <button class="btn" onclick={() => (confirming = false)}>Cancel</button>
    <button class="btn primary" onclick={apply}><Icon name="download" /> Update {plural(chosen.length, "app")}</button>
  {/snippet}
</Modal>

<style>
  h2 {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    margin: 0;
    font-size: 15px;
  }
  .small {
    font-size: 12px;
  }
  .missing p {
    margin: 8px 0 0;
  }
  .card-head {
    display: flex;
    gap: 12px;
    align-items: center;
    margin-bottom: 6px;
  }
  .running-head {
    display: flex;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  .running p {
    margin: 6px 0 0;
  }
  .result {
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-color: var(--tier-green);
  }
  table {
    width: 100%;
    border-collapse: collapse;
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
  .pick {
    width: 28px;
  }
  .ok,
  .fail {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    font-weight: 600;
  }
  .ok {
    color: var(--tier-green);
  }
  .fail {
    color: var(--tier-red);
  }
  .confirm {
    margin: 4px 0;
    padding-left: 20px;
  }
  .bar {
    position: sticky;
    bottom: 0;
    margin-top: auto;
    display: flex;
    gap: 12px;
    align-items: center;
    justify-content: flex-end;
    margin-left: -20px;
    margin-right: -20px;
    margin-bottom: -16px;
    padding: 10px 20px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
</style>
