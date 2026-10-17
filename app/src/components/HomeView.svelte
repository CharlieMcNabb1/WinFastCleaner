<script lang="ts">
  import { untrack } from "svelte";

  import { type Backend, backend, errorMessage } from "../lib/backend.ts";
  import { plural, sizeAndCount } from "../lib/cleaner.ts";
  import { signedSize, splitPath } from "../lib/files.ts";
  import { formatDateTime, formatPercent, formatSize, percent } from "../lib/format.ts";
  import { junkScan, safeTotals, scanJunk } from "../lib/junk.svelte.ts";
  import { driveStatus, FIX_LABEL, latestPair, sortChecks, topGrowth } from "../lib/system.ts";
  import type { ChangeView, DriveInfo, Fix, HealthThresholds, HealthView } from "../lib/types.ts";
  import Icon from "./Icon.svelte";
  import StatusBadge from "./StatusBadge.svelte";

  let {
    active,
    diskScans,
    onNavigate,
    onCleanSafe,
    onShowInDisk,
  }: {
    active: boolean;
    /** Changes when a Disk scan finishes (a new snapshot exists). */
    diskScans: number;
    onNavigate: (tab: Fix | "grew") => void;
    onCleanSafe: () => void;
    onShowInDisk: (path: string, root: string) => void;
  } = $props();

  let api = $state<Backend | null>(null);
  let health = $state<HealthView | null>(null);
  let checking = $state(false);
  let drives = $state<DriveInfo[]>([]);
  let thresholds = $state<HealthThresholds | null>(null);
  let grew = $state<{ root: string; since: number; items: ChangeView[] } | null>(null);
  let grewNote = $state<string | null>(null);
  let error = $state<string | null>(null);

  let checks = $derived(health ? sortChecks(health.checks) : []);
  let attention = $derived(checks.filter((c) => c.status === "red" || c.status === "yellow").length);
  let junkTotal = $derived(
    junkScan.view
      ? {
          size: junkScan.view.rules.reduce((s, r) => s + r.size, 0),
          count: junkScan.view.rules.reduce((s, r) => s + r.files, 0),
        }
      : null,
  );
  let safe = $derived(junkScan.view ? safeTotals(junkScan.view) : null);
  let fixedDrives = $derived(drives.filter((d) => d.kind === "fixed" || d.kind === "removable"));

  // Every visit refreshes the health check (other tabs change what it sees).
  // The first one also looks for junk.
  let started = false;
  $effect(() => {
    if (!active) return;
    untrack(() => {
      if (!started) {
        started = true;
        if (!junkScan.view && !junkScan.scanning) void scanJunk();
      }
      void refresh();
    });
  });

  // A finished junk scan or Disk scan changes the junk and "What grew" cards.
  let lastJunk: unknown = null;
  $effect(() => {
    const v = junkScan.view;
    if (v && v !== lastJunk) {
      lastJunk = v;
      untrack(() => void loadHealth());
    }
  });
  $effect(() => {
    if (diskScans > 0) untrack(() => void loadGrew());
  });

  async function refresh() {
    api ??= await backend();
    await Promise.all([loadHealth(), loadDrives(), loadGrew()]);
  }

  async function loadHealth() {
    api ??= await backend();
    checking = true;
    try {
      const [h, s] = await Promise.all([api.healthCheck(), api.getSettings()]);
      health = h;
      thresholds = s.settings.health;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      checking = false;
    }
  }

  async function loadDrives() {
    api ??= await backend();
    try {
      drives = await api.listDrives();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function loadGrew() {
    api ??= await backend();
    try {
      const all = drives.length > 0 ? drives : await api.listDrives();
      const root = (all.find((d) => d.isSystem) ?? all[0])?.root;
      if (!root) return;
      const pair = latestPair(await api.listSnapshots(), root);
      if (!pair) {
        grew = null;
        grewNote = "After the next scan of this drive, the folders that grew since today will show here.";
        return;
      }
      const cmp = await api.compareSnapshots(pair[1].key, pair[0].key);
      grew = { root, since: cmp.oldTakenAt, items: topGrowth(cmp.grew, 3) };
      grewNote = grew.items.length === 0 ? "Nothing grew noticeably between the last two scans." : null;
    } catch (e) {
      grewNote = errorMessage(e);
    }
  }
</script>

<section class="view home">
  <header class="view-head">
    <div>
      <h1>Home</h1>
      <p class="muted">
        {#if health}
          {attention === 0 ? "Nothing needs your attention." : `${plural(attention, "thing needs", "things need")} your attention.`}
        {:else}
          Checking your PC…
        {/if}
      </p>
    </div>
    <button class="btn" onclick={refresh} disabled={checking}><Icon name="refresh" /> Refresh</button>
  </header>

  {#if error}
    <div class="alert" role="alert">
      <Icon name="alert" size={18} />
      <span>{error}</span>
      <button class="btn ghost" aria-label="Dismiss" onclick={() => (error = null)}><Icon name="x" /></button>
    </div>
  {/if}

  <div class="grid">
    <section class="card junk" aria-labelledby="home-junk">
      <h2 id="home-junk"><Icon name="sparkle" size={16} /> Junk</h2>
      {#if junkScan.scanning && !junkScan.view}
        <p class="muted">Looking for junk…</p>
        <div class="progress-track" role="progressbar" aria-label="Scanning for junk">
          <span class="progress-fill indeterminate"></span>
        </div>
      {:else if junkTotal && safe}
        <p class="big num">{formatSize(junkTotal.size)}</p>
        <p class="muted">can be cleaned ({plural(junkTotal.count, "file")}).</p>
        <div class="actions">
          <button class="btn primary" disabled={safe.count === 0 || junkScan.scanning} onclick={onCleanSafe}>
            <Icon name="sparkle" />
            {safe.count === 0 ? "No safe items to clean" : `Clean safe items: ${sizeAndCount(safe.size, safe.count)}`}
          </button>
          <button class="btn" onclick={() => onNavigate("cleaner")}>Review all</button>
        </div>
        <p class="muted small">You'll see the exact list before anything is removed.</p>
      {:else if junkScan.error}
        <p class="warn">{junkScan.error}</p>
        <button class="btn" onclick={() => scanJunk()}><Icon name="refresh" /> Try again</button>
      {/if}
    </section>

    <section class="card drives" aria-labelledby="home-drives">
      <h2 id="home-drives"><Icon name="drive" size={16} /> Drives</h2>
      <ul>
        {#each fixedDrives as d (d.root)}
          {@const status = thresholds ? driveStatus(d, thresholds) : "unknown"}
          <li>
            <div class="drive-head">
              <strong>{d.root.replace(/\\$/, "")}</strong>
              {#if d.label}<span class="muted">{d.label}</span>{/if}
              {#if status === "yellow" || status === "red"}<StatusBadge {status} />{/if}
              <span class="muted num right">{formatSize(d.free)} free of {formatSize(d.total)}</span>
            </div>
            <span
              class="bar-track"
              role="meter"
              aria-label={`${d.root} used space`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round(percent(d.total - d.free, d.total))}
              aria-valuetext={`${formatPercent(d.free, d.total)} free`}
            >
              <span class="bar-fill {status}" style:width={`${percent(d.total - d.free, d.total)}%`}></span>
            </span>
          </li>
        {/each}
      </ul>
      <button class="link-btn" onclick={() => onNavigate("disk")}>See what's using space</button>
    </section>
  </div>

  <section class="card" aria-labelledby="home-health">
    <div class="card-head">
      <h2 id="home-health"><Icon name="pulse" size={16} /> Health check</h2>
      {#if health}<StatusBadge status={health.overall} />{/if}
      {#if checking}<span class="muted small">Checking…</span>{/if}
    </div>
    <p class="muted small">Read-only: the health check never changes anything. Each problem links to where it can be fixed.</p>
    <ul class="checks">
      {#each checks as c (c.id)}
        <li>
          <StatusBadge status={c.status} />
          <div class="what">
            <strong>{c.title}</strong>
            <span>{c.summary}</span>
            {#each c.details as d, i (i)}
              <span class="muted small">{d}</span>
            {/each}
            {#if c.asOf}<span class="muted small">As of {formatDateTime(c.asOf)}</span>{/if}
          </div>
          {#if c.fix}
            <button class="btn" onclick={() => onNavigate(c.fix!)}>{FIX_LABEL[c.fix]}</button>
          {/if}
        </li>
      {/each}
    </ul>
  </section>

  <section class="card" aria-labelledby="home-grew">
    <div class="card-head">
      <h2 id="home-grew"><Icon name="trend" size={16} /> What grew</h2>
      {#if grew}
        <span class="muted small">on {grew.root.replace(/\\$/, "")} since {formatDateTime(grew.since)}</span>
      {/if}
      <button class="link-btn right" onclick={() => onNavigate("grew")}>See all</button>
    </div>
    {#if grew && grew.items.length > 0}
      <ol class="grew">
        {#each grew.items as g (g.path)}
          {@const p = splitPath(g.path)}
          <li>
            <span class="delta num">{signedSize(g.delta)}</span>
            <span class="ellipsis" title={g.path}><strong>{p.name}</strong> <span class="muted">{p.folder}</span></span>
            <button class="link-btn" onclick={() => onShowInDisk(g.path, grew!.root)}>Show in Disk</button>
          </li>
        {/each}
      </ol>
    {/if}
    {#if grewNote}<p class="muted">{grewNote}</p>{/if}
  </section>
</section>

<style>
  h2 {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    margin: 0 0 6px;
    font-size: 15px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 12px;
  }
  .big {
    margin: 4px 0 0;
    font-size: 28px;
    font-weight: 600;
    text-align: left;
  }
  .junk p {
    margin: 0 0 6px;
  }
  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin: 8px 0 6px;
  }
  .small {
    font-size: 12px;
  }
  .warn {
    color: var(--tier-yellow);
  }
  .drives ul,
  .checks,
  .grew {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .drives li {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 10px;
  }
  .drive-head {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .right {
    margin-left: auto;
  }
  .bar-fill.yellow {
    background: var(--tier-yellow);
  }
  .bar-fill.red {
    background: var(--tier-red);
  }
  .card-head {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .card-head h2 {
    margin: 0;
  }
  .checks li {
    display: grid;
    grid-template-columns: 96px minmax(0, 1fr) auto;
    gap: 12px;
    align-items: start;
    padding: 8px 0;
    border-top: 1px solid var(--surface-2);
  }
  .what {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .grew li {
    display: grid;
    grid-template-columns: 90px minmax(0, 1fr) auto;
    gap: 12px;
    align-items: center;
    padding: 6px 0;
    border-top: 1px solid var(--surface-2);
  }
  .delta {
    color: var(--tier-yellow);
    font-weight: 600;
    text-align: left;
  }
</style>
