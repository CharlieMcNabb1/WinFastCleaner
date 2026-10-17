<script lang="ts">
  import { untrack } from "svelte";

  import { type Backend, backend, errorMessage } from "../lib/backend.ts";
  import { plural, sizeAndCount } from "../lib/cleaner.ts";
  import { formatDateTime } from "../lib/format.ts";
  import { describe, nextRun, parseTime, timeValue, WEEKDAYS } from "../lib/schedule.ts";
  import type { AppInfo, Schedule, ScheduleView, TrayView } from "../lib/types.ts";
  import Icon from "./Icon.svelte";
  import TierBadge from "./TierBadge.svelte";

  let {
    info,
    active,
    onNavigate,
  }: { info: AppInfo | null; active: boolean; onNavigate: (tab: "recover") => void } = $props();

  let api = $state<Backend | null>(null);
  let view = $state<ScheduleView | null>(null);
  let draft = $state<Schedule | null>(null);
  let tray = $state<TrayView | null>(null);
  let saving = $state(false);
  let error = $state<string | null>(null);

  let isWindows = $derived(info?.platform === "windows");
  let changed = $derived(view !== null && draft !== null && JSON.stringify(view.schedule) !== JSON.stringify(draft));
  let chooseRules = $state(false);
  let ruleChoices = $derived(
    view && draft ? view.rules.filter((r) => r.tier === "green" || (draft!.includeOptional && r.tier === "yellow")) : [],
  );
  let next = $derived(view?.schedule.enabled ? nextRun(view.schedule, new Date()) : null);

  $effect(() => {
    if (active) untrack(() => void load());
  });

  async function load() {
    api ??= await backend();
    try {
      const [v, t] = await Promise.all([api.scheduleStatus(), api.trayStatus()]);
      setView(v);
      tray = t;
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function setView(v: ScheduleView) {
    view = v;
    draft = structuredClone($state.snapshot(v.schedule)) as Schedule;
    chooseRules = v.schedule.rules.length > 0;
  }

  async function save() {
    if (!api || !draft) return;
    saving = true;
    error = null;
    try {
      const s = structuredClone($state.snapshot(draft)) as Schedule;
      if (!chooseRules) s.rules = [];
      // Only rules the schedule can actually use.
      s.rules = s.rules.filter((id) => ruleChoices.some((r) => r.id === id));
      setView(await api.scheduleSave(s));
    } catch (e) {
      error = errorMessage(e);
    } finally {
      saving = false;
    }
  }

  function revert() {
    if (view) setView(view);
  }

  function setTime(v: string) {
    const t = parseTime(v);
    if (t && draft) {
      draft.hour = t.hour;
      draft.minute = t.minute;
    }
  }

  function toggleRule(id: string, on: boolean) {
    if (!draft) return;
    draft.rules = on ? [...draft.rules, id] : draft.rules.filter((r) => r !== id);
  }

  async function setTray(on: boolean) {
    if (!api) return;
    error = null;
    try {
      tray = await api.traySet(on);
    } catch (e) {
      error = errorMessage(e);
      tray = await api.trayStatus().catch(() => tray);
    }
  }
</script>

{#if error}
  <div class="alert" role="alert">
    <Icon name="alert" size={18} />
    <span>{error}</span>
    <button class="btn ghost" aria-label="Dismiss" onclick={() => (error = null)}><Icon name="x" /></button>
  </div>
{/if}

{#if view && draft}
  <section class="card" aria-labelledby="set-schedule">
    <h2 id="set-schedule">Scheduled cleaning</h2>
    <p class="muted hint">
      Runs through Windows Task Scheduler as you, without administrator rights, so items that need admin are skipped and
      listed. Review (red) items are never cleaned automatically. If the PC is off at that time, it runs soon after it
      starts.
    </p>

    {#if isWindows && !view.programFound}
      <p class="warn"><Icon name="alert" size={14} /> The scheduled cleaner (wfcw.exe) is missing. Reinstall WinFastCleaner.</p>
    {:else if view.schedule.enabled && view.taskExists === false}
      <p class="warn">
        <Icon name="alert" size={14} /> The task is missing from Task Scheduler. Save the schedule to create it again.
      </p>
    {/if}

    <label class="check">
      <input type="checkbox" bind:checked={draft.enabled} />
      <span>Clean automatically</span>
    </label>

    <fieldset class="grid" disabled={!draft.enabled}>
      <label for="sch-freq">How often</label>
      <span class="row">
        <select id="sch-freq" bind:value={draft.frequency}>
          <option value="daily">Every day</option>
          <option value="weekly">Every week</option>
        </select>
        {#if draft.frequency === "weekly"}
          <select aria-label="Day of the week" bind:value={draft.weekday}>
            {#each WEEKDAYS as d (d.id)}<option value={d.id}>{d.label}</option>{/each}
          </select>
        {/if}
        at
        <input aria-label="Time of day" type="time" value={timeValue(draft)} onchange={(e) => setTime(e.currentTarget.value)} />
      </span>

      <span>What to clean</span>
      <span class="stack">
        <label class="check">
          <input type="radio" name="sch-what" checked={!draft.includeOptional} onchange={() => (draft!.includeOptional = false)} />
          <span><TierBadge tier="green" /> Safe items only (recommended)</span>
        </label>
        <label class="check">
          <input type="radio" name="sch-what" checked={draft.includeOptional} onchange={() => (draft!.includeOptional = true)} />
          <span>
            <TierBadge tier="green" /> and <TierBadge tier="yellow" /> Safe and optional items
            <span class="muted hint">Optional items go to the recovery bin, so they can be restored.</span>
          </span>
        </label>
      </span>

      <span>Rules</span>
      <span class="stack">
        <label class="check">
          <input type="radio" name="sch-rules" checked={!chooseRules} onchange={() => (chooseRules = false)} />
          <span>Every rule of those kinds</span>
        </label>
        <label class="check">
          <input type="radio" name="sch-rules" checked={chooseRules} onchange={() => (chooseRules = true)} />
          <span>Only the rules I pick</span>
        </label>
        {#if chooseRules}
          <ul class="rules" aria-label="Rules to clean on the schedule">
            {#each ruleChoices as r (r.id)}
              <li>
                <label class="check">
                  <input type="checkbox" checked={draft.rules.includes(r.id)} onchange={(e) => toggleRule(r.id, e.currentTarget.checked)} />
                  <span><TierBadge tier={r.tier} /> {r.name} <span class="muted">· {r.group}</span></span>
                </label>
              </li>
            {/each}
          </ul>
        {/if}
      </span>
    </fieldset>

    <div class="actions">
      <button class="btn primary" onclick={save} disabled={!changed || saving || (chooseRules && draft.enabled && draft.rules.length === 0)}>
        <Icon name="check" /> Save schedule
      </button>
      {#if changed}<button class="btn" onclick={revert} disabled={saving}>Undo changes</button>{/if}
      <span class="muted">
        {#if view.schedule.enabled}
          {describe(view.schedule)}.{next ? ` Next run: ${formatDateTime(Math.floor(next.getTime() / 1000))}.` : ""}
        {:else}
          Off.
        {/if}
      </span>
    </div>

    {#if view.last}
      {@const l = view.last}
      <div class="last" class:bad={!l.ok}>
        <strong>Last run {formatDateTime(l.finishedAt || l.startedAt)}:</strong>
        {l.message}
        {#if l.deletedCount + l.binnedCount > 0}
          <span class="muted">
            ({sizeAndCount(l.deletedBytes, l.deletedCount)} deleted{#if l.binnedCount > 0}, {sizeAndCount(l.binnedBytes, l.binnedCount)} moved to the recovery bin{/if}{#if l.skippedCount > 0}, {plural(l.skippedCount, "file")} skipped{/if}.)
          </span>
        {/if}
        {#if l.notReady.length > 0}
          <ul>
            {#each l.notReady as n, i (i)}<li>Skipped {n.rule}: {n.reason}.</li>{/each}
          </ul>
        {/if}
        {#if l.batchId}<button class="link-btn" onclick={() => onNavigate("recover")}>Open the recovery bin</button>{/if}
      </div>
    {/if}
  </section>
{/if}

{#if tray}
  <section class="card" aria-labelledby="set-tray">
    <h2 id="set-tray">Tray monitor</h2>
    <label class="check">
      <input type="checkbox" checked={tray.enabled} onchange={(e) => setTray(e.currentTarget.checked)} />
      <span>
        Show the tray monitor (starts when you sign in)
        <span class="muted hint">
          Checks your PC every hour and notifies you when a drive is low on space, junk piles up, or a health check turns
          red, each at most once a day, using the health check thresholds below. It only looks; it never cleans anything by
          itself.
        </span>
      </span>
    </label>
    {#if tray.enabled}
      <p class="muted hint">{tray.running ? "Running now." : "Not running right now; it starts at your next sign-in."}</p>
    {/if}
    {#if isWindows && !tray.programFound}
      <p class="warn"><Icon name="alert" size={14} /> The tray monitor (wfc-tray.exe) is missing. Reinstall WinFastCleaner.</p>
    {/if}
  </section>
{/if}

<style>
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  p {
    margin: 0;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .hint {
    font-size: 12px;
  }
  .warn {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    color: var(--tier-yellow);
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }
  .check > span {
    display: flex;
    flex-direction: column;
  }
  .check > span:has(:global(.tier)) {
    display: inline;
  }
  .check > span:has(:global(.tier)) .hint {
    display: block;
  }
  fieldset {
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }
  fieldset:disabled {
    opacity: 0.6;
  }
  .grid {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 10px 14px;
    align-items: start;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .rules {
    list-style: none;
    margin: 4px 0 0;
    padding: 6px 8px;
    max-height: 220px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .rules li {
    padding: 2px 0;
  }
  .actions {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }
  .last {
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--surface-2);
  }
  .last.bad {
    background: var(--alert-bg);
  }
  .last ul {
    margin: 4px 0;
    padding-left: 20px;
  }
</style>
