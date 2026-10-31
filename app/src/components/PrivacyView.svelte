<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";

  import { type Backend, backend, errorMessage } from "../lib/backend.ts";
  import { plural } from "../lib/cleaner.ts";
  import type { Trace, TraceInfo, TraceResult } from "../lib/types.ts";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";

  let { active }: { active: boolean } = $props();

  let api = $state<Backend | null>(null);
  let traces = $state<TraceInfo[]>([]);
  // Nothing is ticked by default.
  const picked = new SvelteSet<Trace>();
  let confirming = $state(false);
  let working = $state(false);
  let results = $state<TraceResult[] | null>(null);
  let error = $state<string | null>(null);

  let chosen = $derived(traces.filter((t) => picked.has(t.trace)));

  $effect(() => {
    if (active) void load();
  });

  async function load() {
    api ??= await backend();
    try {
      traces = await api.privacySurvey();
      for (const t of [...picked]) if (!traces.some((x) => x.trace === t && x.available)) picked.delete(t);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function clear() {
    confirming = false;
    if (!api || chosen.length === 0) return;
    working = true;
    error = null;
    try {
      results = await api.privacyClear(chosen.map((t) => t.trace));
      picked.clear();
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      working = false;
    }
  }

  function nameOf(t: Trace): string {
    return traces.find((x) => x.trace === t)?.name ?? t;
  }

  function itemsText(t: TraceInfo): string {
    if (!t.available || t.items === null) return "Not available";
    if (t.trace === "clipboard") return t.items > 0 ? "Has content" : "Empty";
    return t.items === 0 ? "Nothing to clear" : plural(t.items, "item");
  }
</script>

<section class="view">
  <header class="view-head">
    <div>
      <h1>Privacy</h1>
      <p class="muted">
        Clears traces of what you did on this PC. This is separate from freeing space, nothing is selected for you, and
        the contents of what is cleared are never read.
      </p>
    </div>
    <button class="btn" onclick={load} disabled={working}><Icon name="refresh" /> Refresh</button>
  </header>

  {#if error}
    <div class="alert" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button class="btn ghost" aria-label="Dismiss" onclick={() => (error = null)}><Icon name="x" /></button>
    </div>
  {/if}

  {#if results}
    <section class="card result" role="status" aria-label="Privacy cleaning result">
      <div class="head">
        <h2><Icon name="check" size={18} /> Done</h2>
        <button class="btn ghost" aria-label="Dismiss" onclick={() => (results = null)}><Icon name="x" /></button>
      </div>
      <ul>
        {#each results as r (r.trace)}
          <li>
            <strong>{nameOf(r.trace)}:</strong>
            {#if r.error}
              <span class="warn">{r.error}</span>
            {:else if r.trace === "clipboard"}
              emptied.
            {:else}
              cleared {plural(r.cleared, "item")}.
            {/if}
            {#if r.skipped.length > 0}
              <span class="muted">{plural(r.skipped.length, "item was", "items were")} skipped ({r.skipped[0]!.reason}).</span>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  <section class="card" aria-labelledby="windows-traces">
    <h2 id="windows-traces">Windows</h2>
    <ul class="traces">
      {#each traces as t (t.trace)}
        <li class:off={!t.available}>
          <input
            type="checkbox"
            checked={picked.has(t.trace)}
            disabled={!t.available || working}
            aria-label={`Clear ${t.name}`}
            onchange={(e) => (e.currentTarget.checked ? picked.add(t.trace) : picked.delete(t.trace))}
          />
          <div class="what">
            <strong>{t.name}</strong>
            <span class="muted small">{t.description}</span>
            <span class="muted small">What you lose: {t.lose}</span>
          </div>
          <span class="muted count">{itemsText(t)}</span>
        </li>
      {/each}
    </ul>
  </section>

  <p class="muted small">
    Browser history, cookies, and sessions aren't part of this yet. Browser caches are in the Cleaner tab.
  </p>

  <footer class="bar">
    <span class="muted">{chosen.length > 0 ? `${plural(chosen.length, "item")} picked` : "Nothing picked"}</span>
    <button class="btn primary" onclick={() => (confirming = true)} disabled={chosen.length === 0 || working}>
      <Icon name="shield" /> Clear selected
    </button>
  </footer>
</section>

<Modal open={confirming} title="Clear these traces?" onClose={() => (confirming = false)}>
  <ul class="confirm">
    {#each chosen as t (t.trace)}
      <li><strong>{t.name}</strong> ({itemsText(t).toLowerCase()}). <span class="muted">{t.lose}</span></li>
    {/each}
  </ul>
  <p class="muted">These are cleared directly; they don't go to the recovery bin.</p>
  {#snippet actions()}
    <button class="btn" onclick={() => (confirming = false)}>Cancel</button>
    <button class="btn primary" onclick={clear}><Icon name="shield" /> Clear {plural(chosen.length, "item")}</button>
  {/snippet}
</Modal>

<style>
  h2 {
    margin: 0 0 6px;
    font-size: 15px;
  }
  .traces {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .traces li {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    gap: 12px;
    align-items: center;
    padding: 8px 0;
    border-top: 1px solid var(--surface-2);
  }
  .traces li.off {
    opacity: 0.6;
  }
  .what {
    display: flex;
    flex-direction: column;
  }
  .small {
    font-size: 12px;
  }
  .count {
    font-size: 13px;
    white-space: nowrap;
  }
  .result {
    border-color: var(--border);
  }
  .result .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .result h2 {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    margin: 0;
    color: var(--text);
  }
  .result ul,
  .confirm {
    margin: 4px 0;
    padding-left: 20px;
  }
  .warn {
    color: var(--text);
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
