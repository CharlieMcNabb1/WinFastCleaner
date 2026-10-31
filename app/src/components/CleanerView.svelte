<script lang="ts">
  import { untrack } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";

  import { type Backend, backend, errorMessage } from "../lib/backend.ts";
  import {
    type Deselected,
    groupRules,
    isReady,
    plural,
    selectedByDefault,
    selectionTotals,
    sizeAndCount,
    statusText,
  } from "../lib/cleaner.ts";
  import { formatDuration, formatSize } from "../lib/format.ts";
  import { cancelJunkScan, junkScan, scanJunk } from "../lib/junk.svelte.ts";
  import type { AppInfo, CleanerFocus, CleanReport, JunkItem, JunkView, PlanView, RuleView } from "../lib/types.ts";
  import CleanResult from "./CleanResult.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import RuleRow from "./RuleRow.svelte";

  let {
    info,
    active,
    focus,
    onNavigate,
  }: {
    info: AppInfo | null;
    active: boolean;
    /**
     * A rule to scroll to and highlight (from "What grew"), or "safe" to pick
     * only the safe (green) items and show the preview (from Home).
     */
    focus: CleanerFocus | null;
    onNavigate: (tab: "recover") => void;
  } = $props();

  let api = $state<Backend | null>(null);
  /** The scan the selection belongs to (the shared scan, once applied). */
  let junk = $state<JunkView | null>(null);
  let scanning = $derived(junkScan.scanning);
  let error = $state<string | null>(null);
  const selected = new SvelteSet<string>();
  const deselected: Deselected = new SvelteMap();

  let plan = $state<PlanView | null>(null);
  /** Secure delete is offered (Settings) and chosen for this clean. */
  let secureOffered = $state(false);
  let secure = $state(false);
  let preparing = $state(false);
  let cleaning = $state<{ done: number; total: number } | null>(null);
  let result = $state<CleanReport | null>(null);

  let deepConfirm = $state(false);
  let deep = $state<{ percent: number | null } | null>(null);
  let deepResult = $state<string | null>(null);

  let groups = $derived(junk ? groupRules(junk.rules) : []);
  let totals = $derived(junk ? selectionTotals(junk.rules, selected, deselected) : { size: 0, count: 0 });
  let busy = $derived(scanning || preparing || cleaning !== null);
  let needsAdmin = $derived(junk?.rules.filter((r) => r.status.kind === "needsAdmin") ?? []);
  let isWindows = $derived(info?.platform === "windows");
  let elevated = $derived(info?.elevated ?? false);

  let highlighted = $state<string | null>(null);
  let lastFocus = 0;
  $effect(() => {
    const f = focus;
    if (!f || f.seq === lastFocus || !junk) return;
    lastFocus = f.seq;
    if (f.kind === "safe") {
      untrack(() => pickSafe());
      return;
    }
    highlighted = f.ruleId;
    // Wait for the row to render, then bring it into view.
    requestAnimationFrame(() =>
      document.getElementById(`rule-${f.ruleId}`)?.scrollIntoView({ block: "center", behavior: "smooth" }),
    );
    setTimeout(() => {
      if (highlighted === f.ruleId) highlighted = null;
    }, 2500);
  });

  // The first visit to the tab scans, unless Home already did.
  let started = false;
  $effect(() => {
    if (active && !started) {
      started = true;
      void (async () => {
        api ??= await backend();
        if (!untrack(() => junkScan.view || junkScan.scanning)) await scan();
      })();
    }
  });

  // Each finished scan (from here or from Home) replaces the results.
  $effect(() => {
    const view = junkScan.view;
    if (view && view !== untrack(() => junk)) untrack(() => applyResults(view));
  });
  $effect(() => {
    if (junkScan.error) error = junkScan.error;
  });

  async function scan() {
    api ??= await backend();
    await scanJunk();
  }

  /** Only the safe items, then straight to the preview. */
  function pickSafe() {
    if (!junk) return;
    selected.clear();
    deselected.clear();
    for (const r of junk.rules) if (selectedByDefault(r)) selected.add(r.id);
    if (selected.size > 0) void preview();
  }

  function applyResults(view: JunkView) {
    // Keep the user's choices for rules that are still there.
    const previous = junk ? new Set(selected) : null;
    selected.clear();
    deselected.clear();
    for (const r of view.rules) {
      const keep = previous ? previous.has(r.id) : selectedByDefault(r);
      if (keep && isReady(r)) selected.add(r.id);
    }
    junk = view;
  }

  function toggleRule(rule: RuleView, on: boolean) {
    if (on) selected.add(rule.id);
    else selected.delete(rule.id);
  }

  function toggleGroup(rules: RuleView[], on: boolean) {
    for (const r of rules) if (isReady(r)) toggleRule(r, on);
  }

  function toggleFile(rule: RuleView, item: JunkItem, on: boolean) {
    let off = deselected.get(rule.id);
    if (!off) {
      off = new SvelteMap();
      deselected.set(rule.id, off);
    }
    if (on) off.delete(item.index);
    else off.set(item.index, item.size);
  }

  async function preview() {
    api ??= await backend();
    if (!junk) return;
    preparing = true;
    error = null;
    try {
      const selection = junk.rules
        .filter((r) => selected.has(r.id) && isReady(r))
        .map((r) => ({ ruleId: r.id, deselected: [...(deselected.get(r.id)?.keys() ?? [])] }));
      const [preview, settings] = await Promise.all([api.cleanPreview(junk.scanId, selection), api.getSettings()]);
      secureOffered = settings.settings.secureDelete;
      secure = false;
      plan = preview;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      preparing = false;
    }
  }

  async function clean() {
    if (!api || !plan) return;
    const planId = plan.planId;
    plan = null;
    result = null;
    cleaning = { done: 0, total: 0 };
    try {
      await api.cleanExecute(planId, secure, (e) => {
        switch (e.type) {
          case "progress":
            cleaning = { done: e.done, total: e.total };
            break;
          case "done":
            cleaning = null;
            result = e.report;
            void scan();
            break;
          case "failed":
            cleaning = null;
            error = e.message;
            break;
        }
      });
    } catch (e) {
      cleaning = null;
      error = errorMessage(e);
    }
  }

  async function restartAsAdmin() {
    try {
      await (api ?? (await backend())).relaunchElevated();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function runDeepClean() {
    deepConfirm = false;
    if (!api) return;
    deepResult = null;
    deep = { percent: null };
    try {
      await api.deepClean((e) => {
        switch (e.type) {
          case "progress":
            deep = { percent: e.percent };
            break;
          case "done":
            deep = null;
            deepResult =
              `Deep clean finished and freed ${formatSize(e.freedBytes)}.` +
              (e.restartNeeded ? " Restart Windows to finish." : "");
            break;
          case "failed":
            deep = null;
            error = e.message;
            break;
        }
      });
    } catch (e) {
      deep = null;
      error = errorMessage(e);
    }
  }

  function groupState(rules: RuleView[]): { checked: boolean; mixed: boolean; any: boolean } {
    const ready = rules.filter(isReady);
    const on = ready.filter((r) => selected.has(r.id)).length;
    return { checked: on > 0 && on === ready.length, mixed: on > 0 && on < ready.length, any: ready.length > 0 };
  }
</script>

<section class="view cleaner">
  <header class="view-head">
    <div>
      <h1>Cleaner</h1>
      <p class="muted">
        Junk found by rules you can read. Nothing is removed until you confirm the exact list, and protected Windows files
        are never shown or touched.
      </p>
    </div>
    <button class="btn" onclick={scan} disabled={busy}><Icon name="refresh" /> Scan again</button>
  </header>

  {#if isWindows && !elevated && needsAdmin.length > 0}
    <div class="notice">
      <Icon name="shield" size={18} />
      <span>
        <strong>{plural(needsAdmin.length, "item needs", "items need")} administrator rights</strong>
        ({formatSize(needsAdmin.reduce((s, r) => s + r.size, 0))}, such as Windows' own temporary files). They are
        skipped unless you restart as administrator.
      </span>
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

  {#if result}
    <CleanResult report={result} onRecover={() => onNavigate("recover")} onDismiss={() => (result = null)} />
  {/if}

  {#if cleaning}
    <section class="card running" aria-live="polite">
      <div class="running-head">
        <strong>Cleaning…</strong>
        <span class="muted">
          {cleaning.total > 0 ? `${cleaning.done.toLocaleString()} of ${plural(cleaning.total, "file")}` : "Starting"}
        </span>
        <button class="btn" onclick={() => api?.cancelClean()}><Icon name="x" /> Cancel</button>
      </div>
      <div
        class="progress-track"
        role="progressbar"
        aria-label="Cleaning progress"
        aria-valuemin={0}
        aria-valuemax={cleaning.total}
        aria-valuenow={cleaning.done}
      >
        <span class="progress-fill" style:width={`${cleaning.total ? (cleaning.done * 100) / cleaning.total : 0}%`}></span>
      </div>
    </section>
  {/if}

  {#if scanning}
    <section class="card running" aria-live="polite">
      <div class="running-head">
        <strong>Looking for junk…</strong>
        <button class="btn" onclick={cancelJunkScan}><Icon name="x" /> Cancel</button>
      </div>
      <div class="progress-track" role="progressbar" aria-label="Scanning for junk">
        <span class="progress-fill indeterminate"></span>
      </div>
    </section>
  {/if}

  {#if junk}
    <p class="muted meta">
      Found {sizeAndCount(junk.rules.reduce((s, r) => s + r.size, 0), junk.rules.reduce((s, r) => s + r.files, 0))}
      in {formatDuration(junk.durationMs)}.
    </p>

    {#if junk.errors.length > 0}
      <details class="card rule-errors">
        <summary><Icon name="alert" size={14} /> {plural(junk.errors.length, "rule problem")} (those rules were not used)</summary>
        <ul>
          {#each junk.errors as e, i (i)}
            <li><span class="mono">{e.source}</span>{#if e.rule}, rule <strong>{e.rule}</strong>{/if}: {e.message}</li>
          {/each}
        </ul>
      </details>
    {/if}

    {#each groups as g (g.name)}
      {@const gs = groupState(g.rules)}
      <section class="card group" aria-label={g.name}>
        <header class="group-head">
          <input
            type="checkbox"
            checked={gs.checked}
            indeterminate={gs.mixed}
            disabled={!gs.any || busy}
            aria-label={`Clean everything ready in ${g.name}`}
            onchange={(e) => toggleGroup(g.rules, e.currentTarget.checked)}
          />
          <h2>{g.name}</h2>
          <span class="muted num">{formatSize(g.size)}</span>
        </header>
        <ul class="rules">
          {#each g.rules as rule (rule.id)}
            {#if api}
              <RuleRow
                {api}
                scanId={junk.scanId}
                {rule}
                highlight={highlighted === rule.id}
                checked={selected.has(rule.id)}
                deselected={deselected.get(rule.id) as SvelteMap<number, number> | undefined}
                disabled={busy}
                onToggle={(on) => toggleRule(rule, on)}
                onToggleFile={(item, on) => toggleFile(rule, item, on)}
              />
            {/if}
          {/each}
        </ul>
      </section>
    {/each}

    {#if isWindows}
      <section class="card deep" aria-labelledby="deep-title">
        <div>
          <h2 id="deep-title"><Icon name="layers" /> Deep clean</h2>
          <p class="muted">
            Removes old, replaced versions of Windows components with Windows' own tool (DISM component store cleanup). It is
            slow (often 5 to 15 minutes), needs administrator rights, and never runs as part of normal cleaning. Afterwards,
            some older updates can no longer be uninstalled.
          </p>
          {#if deepResult}<p class="ok"><Icon name="check" size={14} /> {deepResult}</p>{/if}
          {#if deep}
            <div class="deep-progress" aria-live="polite">
              <span>Deep clean is running{deep.percent !== null ? `: ${deep.percent.toFixed(1)}%` : "…"}</span>
              <div
                class="progress-track"
                role="progressbar"
                aria-label="Deep clean progress"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={deep.percent ?? undefined}
              >
                {#if deep.percent === null}
                  <span class="progress-fill indeterminate"></span>
                {:else}
                  <span class="progress-fill" style:width={`${deep.percent}%`}></span>
                {/if}
              </div>
              <span class="muted small">
                Windows does this work itself and finishes it safely, so it can't be stopped once started. You can keep using
                other tabs.
              </span>
            </div>
          {/if}
        </div>
        {#if !elevated}
          <button class="btn" onclick={restartAsAdmin}><Icon name="shield" /> Restart as administrator</button>
        {:else}
          <button class="btn" onclick={() => (deepConfirm = true)} disabled={deep !== null}><Icon name="layers" /> Run deep clean</button>
        {/if}
      </section>
    {/if}
  {:else if !scanning && !error}
    <div class="empty muted">Scan to find junk files.</div>
  {/if}

  {#if junk}
    <footer class="bar">
      <span class="muted">
        {#if totals.count > 0}
          Selected: {sizeAndCount(totals.size, totals.count)}
        {:else}
          Nothing selected
        {/if}
      </span>
      <button class="btn primary big" onclick={preview} disabled={totals.count === 0 || busy}>
        <Icon name="sparkle" />
        Clean {sizeAndCount(totals.size, totals.count)}
      </button>
    </footer>
  {/if}
</section>

<Modal open={plan !== null} title="Clean these files?" onClose={() => (plan = null)}>
  {#if plan}
    <p>The exact list was just built from your selection. Only these files are touched.</p>
    <ul class="plan">
      {#if secure}
        <li>
          <strong>{sizeAndCount(plan.totalSize, plan.count)}</strong> overwritten, then deleted. Nothing goes to the recovery
          bin, so these can't be restored.
        </li>
      {:else}
        {#if plan.deleteCount > 0}
          <li>
            <strong>{sizeAndCount(plan.deleteBytes, plan.deleteCount)}</strong> deleted now. These are safe items that
            rebuild on their own.
          </li>
        {/if}
        {#if plan.binCount > 0}
          <li>
            <strong>{sizeAndCount(plan.binBytes, plan.binCount)}</strong> moved to the recovery bin, where you can restore them.
            Their space is freed when the bin is emptied or the batch expires.
          </li>
        {/if}
      {/if}
    </ul>
    {#if plan.notReady.length > 0}
      <p class="muted">
        Skipped:
        {#each plan.notReady as n, i (n.ruleId)}{i > 0 ? "; " : ""}{junk?.rules.find((r) => r.id === n.ruleId)?.name ??
            n.ruleId} ({statusText(n.status).toLowerCase()}){/each}.
      </p>
    {/if}
    {#if secureOffered}
      <label class="secure">
        <input type="checkbox" bind:checked={secure} />
        <span>
          <strong>Secure delete:</strong> overwrite these files before deleting them, so they can't be recovered. Slower;
          nothing goes to the recovery bin, so they can't be restored; less effective on SSDs.
        </span>
      </label>
    {/if}
    <p class="muted small">Files that are in use or changed since the scan are skipped and listed afterwards.</p>
  {/if}
  {#snippet actions()}
    <button class="btn" onclick={() => (plan = null)}>Cancel</button>
    <button class="btn primary" onclick={clean} disabled={!plan || plan.count === 0}>
      <Icon name="sparkle" /> Clean {plan ? sizeAndCount(plan.totalSize, plan.count) : ""}
    </button>
  {/snippet}
</Modal>

<Modal open={deepConfirm} title="Run deep clean?" onClose={() => (deepConfirm = false)}>
  <p>
    Windows will remove old versions of its components that newer updates replaced. This can take 5 to 15 minutes or longer
    and can't be stopped once started.
  </p>
  <p class="muted">Afterwards, some older updates can no longer be uninstalled. Nothing else is changed.</p>
  {#snippet actions()}
    <button class="btn" onclick={() => (deepConfirm = false)}>Cancel</button>
    <button class="btn primary" onclick={runDeepClean}><Icon name="layers" /> Run deep clean</button>
  {/snippet}
</Modal>

<style>
  .cleaner {
    padding-bottom: 0;
  }
  .running {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .running-head {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .running-head button {
    margin-left: auto;
  }
  .meta {
    margin: 0;
  }
  .rule-errors summary {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    cursor: pointer;
    color: var(--text);
  }
  .rule-errors ul {
    margin: 8px 0 0;
    font-size: 12px;
  }
  .group {
    padding: 8px 10px 4px;
  }
  .group-head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 2px 4px 6px;
  }
  .group-head h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    flex: 1;
  }
  .rules {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .deep {
    display: flex;
    gap: 16px;
    align-items: flex-start;
    justify-content: space-between;
  }
  .deep h2 {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    margin: 0 0 4px;
    font-size: 15px;
  }
  .deep p {
    margin: 0;
    max-width: 70ch;
  }
  .deep-progress {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 10px;
  }
  .ok {
    display: flex;
    gap: 6px;
    align-items: center;
    margin-top: 8px !important;
    color: var(--text);
  }
  .small {
    font-size: 12px;
  }
  .empty {
    padding: 40px;
    text-align: center;
  }
  .bar {
    position: sticky;
    bottom: 0;
    display: flex;
    gap: 12px;
    align-items: center;
    justify-content: flex-end;
    margin: 0 -20px;
    padding: 10px 20px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
  .big {
    padding: 8px 16px;
    font-weight: 600;
  }
  .plan {
    margin: 0;
    padding-left: 20px;
  }
  .plan li {
    margin: 4px 0;
  }
  .secure {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 8px 10px;
    border: 1px solid var(--alert-border);
    border-radius: var(--radius);
    background: var(--alert-bg);
  }
</style>
