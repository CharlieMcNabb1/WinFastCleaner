<script lang="ts">
  import { type Backend, backend, errorMessage } from "../lib/backend.ts";
  import { plural } from "../lib/cleaner.ts";
  import { applyTheme } from "../lib/theme.svelte.ts";
  import type {
    AppInfo,
    Exclusion,
    ExclusionKind,
    HealthThresholds,
    Settings,
    SettingsView,
    Theme,
  } from "../lib/types.ts";
  import AutomationSettings from "./AutomationSettings.svelte";
  import Icon, { type IconName } from "./Icon.svelte";

  let {
    info,
    active,
    onNavigate,
  }: { info: AppInfo | null; active: boolean; onNavigate: (tab: "recover") => void } = $props();

  const GB = 1024 ** 3;
  const KIND_ICON: Record<ExclusionKind, IconName> = { folder: "folder", file: "file", pattern: "pattern" };
  const KIND_LABEL: Record<ExclusionKind, string> = { folder: "Folder", file: "File", pattern: "Pattern" };

  let api = $state<Backend | null>(null);
  let view = $state<SettingsView | null>(null);
  let error = $state<string | null>(null);
  let saved = $state(false);
  let pattern = $state("");
  let savedTimer: ReturnType<typeof setTimeout> | undefined;

  let isWindows = $derived(info?.platform === "windows");

  $effect(() => {
    if (active) void load();
  });

  async function load() {
    api ??= await backend();
    try {
      view = await api.getSettings();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  /** Applies a change and saves it right away. Invalid values are refused by
   *  the backend and the previous settings stay. */
  async function update(change: (s: Settings) => void) {
    if (!api || !view) return;
    const next = structuredClone($state.snapshot(view.settings)) as Settings;
    change(next);
    error = null;
    try {
      view = await api.saveSettings(next);
      saved = true;
      clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (saved = false), 2000);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function addExclusion(kind: ExclusionKind, value: string | null) {
    const v = value?.trim();
    if (!v || !view) return;
    if (view.settings.exclusions.some((e) => e.kind === kind && e.value.toLowerCase() === v.toLowerCase())) return;
    void update((s) => s.exclusions.push({ kind, value: v }));
  }

  async function addFolder() {
    if (!api) return;
    try {
      addExclusion("folder", await api.pickFolder("Choose a folder the cleaner must never touch"));
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function addFile() {
    if (!api) return;
    try {
      addExclusion("file", await api.pickFile("Choose a file the cleaner must never touch"));
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function addPattern(e: SubmitEvent) {
    e.preventDefault();
    addExclusion("pattern", pattern);
    pattern = "";
  }

  function remove(ex: Exclusion) {
    void update((s) => {
      s.exclusions = s.exclusions.filter((e) => !(e.kind === ex.kind && e.value === ex.value));
    });
  }

  function setSnapshotDays(raw: string) {
    const n = Number(raw);
    if (!Number.isInteger(n)) return;
    void update((s) => (s.snapshotDays = n));
  }

  function setNumber(field: "maxPercent" | "maxBatches" | "maxAgeDays", raw: string) {
    const n = Number(raw);
    if (!Number.isInteger(n)) return;
    void update((s) => (s.bin[field] = n));
  }

  function setTheme(theme: Theme) {
    applyTheme(theme);
    void update((s) => (s.theme = theme));
  }

  function setHealth(field: keyof HealthThresholds, raw: string) {
    const n = Number(raw);
    if (!Number.isInteger(n)) return;
    void update((s) => (s.health[field] = n));
  }

  function setMaxGb(raw: string) {
    const n = Number(raw);
    if (!Number.isFinite(n) || n <= 0) return;
    void update((s) => (s.bin.maxBytes = Math.round(n * GB)));
  }

  async function openRules() {
    if (!api) return;
    try {
      await api.openRulesFolder();
    } catch (e) {
      error = errorMessage(e);
    }
  }
</script>

<section class="view settings">
  <header class="view-head">
    <div>
      <h1>Settings</h1>
      <p class="muted">Changes are saved as you make them. Everything stays on this PC.</p>
    </div>
    {#if saved}<span class="saved" role="status"><Icon name="check" size={14} /> Saved</span>{/if}
  </header>

  {#if error}
    <div class="alert" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button class="btn ghost" aria-label="Dismiss" onclick={() => (error = null)}><Icon name="x" /></button>
    </div>
  {/if}

  {#if view}
    {@const s = view.settings}
    <section class="card" aria-labelledby="set-theme">
      <h2 id="set-theme">Appearance</h2>
      <div class="grid">
        <label for="theme">Theme</label>
        <span>
          <select id="theme" value={s.theme} onchange={(e) => setTheme(e.currentTarget.value as Theme)}>
            <option value="system">Same as Windows</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </span>
      </div>
    </section>

    <section class="card" aria-labelledby="set-cleaning">
      <h2 id="set-cleaning">Cleaning</h2>
      <label class="check">
        <input type="checkbox" checked={s.greenToBin} onchange={(e) => update((x) => (x.greenToBin = e.currentTarget.checked))} />
        <span>
          Send safe (green) items to the recovery bin too
          <span class="muted hint">
            Off by default: safe items rebuild on their own, so they are deleted directly and the space is freed right away.
          </span>
        </span>
      </label>
    </section>

    {#if isWindows}
      <AutomationSettings {info} {active} {onNavigate} />
    {/if}

    <section class="card" aria-labelledby="set-secure">
      <h2 id="set-secure">Secure delete</h2>
      <label class="check">
        <input type="checkbox" checked={s.secureDelete} onchange={(e) => update((x) => (x.secureDelete = e.currentTarget.checked))} />
        <span>
          Offer secure delete
          <span class="muted hint">
            When it's offered, the Cleaner and the Recover tab let you overwrite files before deleting them, so they can't be
            recovered. It is slower, skips the recovery bin (nothing can be restored), and is less effective on SSDs, which
            may keep old copies of data elsewhere. Files you pick yourself always go to the recovery bin first; delete them
            securely from the Recover tab.
          </span>
        </span>
      </label>
    </section>

    <section class="card" aria-labelledby="set-bin">
      <h2 id="set-bin">Recovery bin</h2>
      <p class="muted hint">
        When a new batch goes over any limit, the oldest batches are deleted for good. The newest batch is always kept.
      </p>
      <div class="grid">
        <label for="bin-gb">Maximum size</label>
        <span><input id="bin-gb" type="number" min="0.1" step="0.5" value={(s.bin.maxBytes / GB).toFixed(1)} onchange={(e) => setMaxGb(e.currentTarget.value)} /> GB</span>
        <label for="bin-pct">Maximum share of a drive</label>
        <span><input id="bin-pct" type="number" min="1" max="50" value={s.bin.maxPercent} onchange={(e) => setNumber("maxPercent", e.currentTarget.value)} /> %
          <span class="muted hint">(the smaller of the two applies)</span></span>
        <label for="bin-batches">Batches to keep</label>
        <span><input id="bin-batches" type="number" min="1" max="100" value={s.bin.maxBatches} onchange={(e) => setNumber("maxBatches", e.currentTarget.value)} /></span>
        <label for="bin-days">Keep batches for</label>
        <span><input id="bin-days" type="number" min="0" max="365" value={s.bin.maxAgeDays} onchange={(e) => setNumber("maxAgeDays", e.currentTarget.value)} /> days</span>
      </div>
    </section>

    <section class="card" aria-labelledby="set-snapshots">
      <h2 id="set-snapshots">Scan snapshots</h2>
      <p class="muted hint">Each scan saves folder sizes for the What grew tab. Older snapshots are deleted.</p>
      <div class="grid">
        <label for="snap-days">Keep snapshots for</label>
        <span><input id="snap-days" type="number" min="1" max="3650" value={s.snapshotDays} onchange={(e) => setSnapshotDays(e.currentTarget.value)} /> days</span>
      </div>
    </section>

    <section class="card" aria-labelledby="set-health">
      <h2 id="set-health">Health check</h2>
      <p class="muted hint">When each check on Home turns yellow (attention) or red (problem).</p>
      <div class="grid">
        <span>Free space on a drive</span>
        <span>
          yellow below <input aria-label="Free space, yellow below (percent)" type="number" min="2" max="99" value={s.health.diskFreeYellowPct} onchange={(e) => setHealth("diskFreeYellowPct", e.currentTarget.value)} /> %,
          red below <input aria-label="Free space, red below (percent)" type="number" min="1" max="98" value={s.health.diskFreeRedPct} onchange={(e) => setHealth("diskFreeRedPct", e.currentTarget.value)} /> %
        </span>
        <span>Junk that can be cleaned</span>
        <span>
          yellow above <input aria-label="Junk, yellow above (GB)" type="number" min="1" value={s.health.junkYellowGb} onchange={(e) => setHealth("junkYellowGb", e.currentTarget.value)} /> GB,
          red above <input aria-label="Junk, red above (GB)" type="number" min="2" value={s.health.junkRedGb} onchange={(e) => setHealth("junkRedGb", e.currentTarget.value)} /> GB
        </span>
        <span>Apps starting at sign-in</span>
        <span>
          yellow above <input aria-label="Startup apps, yellow above" type="number" min="1" value={s.health.startupYellow} onchange={(e) => setHealth("startupYellow", e.currentTarget.value)} />,
          red above <input aria-label="Startup apps, red above" type="number" min="2" max="1000" value={s.health.startupRed} onchange={(e) => setHealth("startupRed", e.currentTarget.value)} />
        </span>
        <label for="health-bin">Recovery bin full</label>
        <span><input id="health-bin" type="number" min="50" max="100" value={s.health.binYellowPct} onchange={(e) => setHealth("binYellowPct", e.currentTarget.value)} /> % of its limit (yellow)</span>
        <label for="health-wear">SSD wear</label>
        <span><input id="health-wear" type="number" min="1" max="100" value={s.health.ssdWearYellowPct} onchange={(e) => setHealth("ssdWearYellowPct", e.currentTarget.value)} /> % of rated life used (yellow)</span>
      </div>
    </section>

    <section class="card" aria-labelledby="set-exclusions">
      <h2 id="set-exclusions">Exclusions (never touch)</h2>
      <p class="muted hint">
        Excluded files and folders are never cleaned, by any rule or selection, and the file finders skip them. You can
        also add them from the Disk tab (right-click an item).
      </p>
      {#if s.exclusions.length === 0}
        <p class="muted">No exclusions yet.</p>
      {:else}
        <ul class="exclusions">
          {#each s.exclusions as ex (ex.kind + ex.value)}
            <li>
              <span class="kind" title={KIND_LABEL[ex.kind]}><Icon name={KIND_ICON[ex.kind]} label={KIND_LABEL[ex.kind]} /></span>
              <span class="mono ellipsis" title={ex.value}>{ex.value}</span>
              <button class="btn ghost" onclick={() => remove(ex)} aria-label={`Remove ${ex.value}`}><Icon name="x" /></button>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="add">
        <button class="btn" onclick={addFolder}><Icon name="folder" /> Add folder…</button>
        <button class="btn" onclick={addFile}><Icon name="file" /> Add file…</button>
        <form class="pattern" onsubmit={addPattern}>
          <label class="sr-only" for="pattern">Pattern</label>
          <input id="pattern" type="text" placeholder="*.psd or node_modules" bind:value={pattern} maxlength="260" />
          <button class="btn" type="submit" disabled={!pattern.trim()}><Icon name="plus" /> Add pattern</button>
        </form>
      </div>
      <p class="muted hint">
        Patterns: <span class="mono">*</span> matches any characters in a name, <span class="mono">?</span> one character.
        A pattern without a slash (like <span class="mono">*.psd</span> or <span class="mono">node_modules</span>) matches that
        name in any folder.
      </p>
    </section>

    <section class="card" aria-labelledby="set-rules">
      <h2 id="set-rules">Custom cleaning rules</h2>
      <p class="muted hint">
        Add your own rule files (.toml) to this folder. They are plain data, checked before use, and can never point at
        protected Windows locations. The format is described in docs/rule-format.md.
      </p>
      <p class="mono ellipsis" title={view.rulesDir}>{view.rulesDir}</p>
      <p>{plural(view.userRules, "custom rule")} loaded.</p>
      {#if view.ruleErrors.length > 0}
        <ul class="rule-errors">
          {#each view.ruleErrors as e, i (i)}
            <li><Icon name="alert" size={13} /> <span class="mono">{e.source}</span>{#if e.rule}, rule <strong>{e.rule}</strong>{/if}: {e.message}</li>
          {/each}
        </ul>
      {/if}
      <button class="btn" onclick={openRules}><Icon name="folderOpen" /> Open rules folder</button>
    </section>

    <section class="card" aria-labelledby="set-data">
      <h2 id="set-data">Your data</h2>
      <p class="muted hint">
        Settings, scan snapshots, the recovery bin index, and the cleanup log are kept here. Nothing is ever sent anywhere.
      </p>
      <p class="mono ellipsis" title={view.dataDir}>{view.dataDir}</p>
    </section>
  {/if}
</section>

<style>
  .settings .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  p {
    margin: 0;
  }
  .hint {
    font-size: 12px;
  }
  .saved {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    color: var(--tier-green);
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
  .grid {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 8px 14px;
    align-items: center;
  }
  .grid input {
    width: 90px;
  }
  .grid span input {
    width: 70px;
  }
  .exclusions {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .exclusions li {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    padding: 2px 0;
    border-bottom: 1px solid var(--surface-2);
  }
  .kind {
    display: inline-flex;
    color: var(--text-2);
  }
  .add {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .pattern {
    display: flex;
    gap: 8px;
  }
  .rule-errors {
    margin: 0;
    padding-left: 4px;
    list-style: none;
    font-size: 12px;
    color: var(--tier-yellow);
  }
  .settings .card > .btn {
    align-self: flex-start;
  }
</style>
