<script lang="ts">
  import type { SvelteMap } from "svelte/reactivity";

  import { type Backend, errorMessage } from "../lib/backend.ts";
  import { isReady, plural, statusText } from "../lib/cleaner.ts";
  import { formatCount, formatDate, formatDateTime, formatSize } from "../lib/format.ts";
  import type { JunkItem, RuleView } from "../lib/types.ts";
  import Icon from "./Icon.svelte";
  import TierBadge from "./TierBadge.svelte";

  let {
    api,
    scanId,
    rule,
    highlight = false,
    checked,
    deselected,
    disabled,
    onToggle,
    onToggleFile,
  }: {
    api: Backend;
    scanId: number;
    rule: RuleView;
    highlight?: boolean;
    checked: boolean;
    /** Files unticked in this rule's preview (index -> size). */
    deselected: SvelteMap<number, number> | undefined;
    disabled: boolean;
    onToggle: (on: boolean) => void;
    onToggleFile: (item: JunkItem, on: boolean) => void;
  } = $props();

  const PAGE = 200;
  let expanded = $state(false);
  let items = $state<JunkItem[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let error = $state<string | null>(null);

  let ready = $derived(isReady(rule));
  let status = $derived(statusText(rule.status));
  let off = $derived(deselected?.size ?? 0);
  const previewId = `preview-${Math.random().toString(36).slice(2)}`;

  // A new scan replaces the file list: reload an open preview.
  let lastScan = -1;
  $effect.pre(() => {
    if (scanId === lastScan) return;
    const first = lastScan === -1;
    lastScan = scanId;
    if (first) return;
    items = [];
    total = 0;
    if (expanded && rule.files > 0) void load(0);
    else expanded = false;
  });

  async function load(offset: number) {
    loading = true;
    error = null;
    try {
      const page = await api.junkItems(scanId, rule.id, offset, PAGE);
      items = offset === 0 ? page.items : [...items, ...page.items];
      total = page.total;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  function toggleExpanded() {
    expanded = !expanded;
    if (expanded && items.length === 0) void load(0);
  }
</script>

<li class="rule" class:muted-row={!ready} class:highlight id={`rule-${rule.id}`}>
  <div class="row">
    <input
      type="checkbox"
      checked={checked && ready}
      disabled={!ready || disabled}
      aria-label={`Clean ${rule.name}`}
      onchange={(e) => onToggle(e.currentTarget.checked)}
    />
    <TierBadge tier={rule.tier} />
    <div class="name">
      <span class="title">
        {rule.name}
        {#if rule.userRule}<span class="tag">your rule</span>{/if}
        {#if rule.requiresAdmin}<span class="tag" title="Needs administrator rights"><Icon name="shield" size={12} /> admin</span>{/if}
      </span>
      <span class="why muted">
        {rule.description}
        <span class="lose">What you lose: {rule.lose}</span>
        {#if rule.minAgeDays}<span class="lose">Only files older than {plural(rule.minAgeDays, "day")}.</span>{/if}
      </span>
    </div>
    <div class="status" class:warn={rule.status.kind === "needsAdmin" || rule.status.kind === "appRunning"}>
      {#if status}
        {#if rule.status.kind !== "nothingFound"}<Icon name="alert" size={14} />{/if}
        {status}
      {/if}
    </div>
    <div class="size num">
      <strong>{formatSize(rule.size)}</strong>
      <span class="muted">{formatCount(rule.files)} {rule.files === 1 ? "file" : "files"}</span>
      {#if off > 0}<span class="muted">{formatCount(off)} unticked</span>{/if}
    </div>
    <button
      class="btn ghost expand"
      onclick={toggleExpanded}
      disabled={rule.files === 0}
      aria-expanded={expanded}
      aria-controls={previewId}
      aria-label={`Show the files in ${rule.name}`}
      title="Show the exact files"
    >
      <Icon name={expanded ? "chevronDown" : "chevronRight"} />
    </button>
  </div>

  {#if expanded}
    <div class="preview" id={previewId}>
      {#if rule.hidden > 0}
        <p class="muted note">
          <Icon name="lock" size={13} />
          {plural(rule.hidden, "matching file")} left out because {rule.hidden === 1 ? "it is" : "they are"} protected, on your
          exclusions list, a link, or a system or cloud file. {rule.hidden === 1 ? "It is" : "They are"} never cleaned.
        </p>
      {/if}
      {#if error}<p class="alert-text">{error}</p>{/if}
      <ul class="files">
        {#each items as item (item.index)}
          <li>
            <input
              type="checkbox"
              checked={!deselected?.has(item.index)}
              disabled={!ready || disabled}
              aria-label={`Clean ${item.path}`}
              onchange={(e) => onToggleFile(item, e.currentTarget.checked)}
            />
            <!-- Long paths are cut on the left so the file name stays visible. -->
            <span class="mono ellipsis path" title={item.path}><bdi>{item.path}</bdi></span>
            <span class="num muted">{formatSize(item.size)}</span>
            <span class="num muted" title={formatDateTime(item.modified)}>{formatDate(item.modified)}</span>
          </li>
        {/each}
      </ul>
      {#if items.length < total}
        <button class="btn" onclick={() => load(items.length)} disabled={loading}>
          Show more ({formatCount(total - items.length)} more)
        </button>
      {:else if loading}
        <p class="muted">Loading…</p>
      {/if}
    </div>
  {/if}
</li>

<style>
  .rule {
    border-top: 1px solid var(--surface-2);
    transition: background 0.4s;
  }
  .rule.highlight {
    background: var(--accent-2);
  }
  .row {
    display: grid;
    grid-template-columns: auto auto minmax(0, 1fr) auto 110px auto;
    gap: 10px;
    align-items: center;
    padding: 8px 4px;
  }
  .muted-row .title {
    color: var(--text-2);
  }
  .name {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .title {
    font-weight: 500;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin-left: 6px;
    padding: 0 6px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--text-2);
    font-size: 11px;
    font-weight: 500;
    vertical-align: 1px;
  }
  .why {
    font-size: 12px;
  }
  .lose {
    margin-left: 4px;
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: var(--text-2);
    text-align: right;
  }
  .status.warn {
    color: var(--text);
  }
  .size {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    font-size: 12px;
  }
  .size strong {
    font-size: 14px;
  }
  .expand {
    padding: 4px 6px;
  }
  .preview {
    margin: 0 4px 10px 34px;
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .note {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    margin: 0 0 6px;
    font-size: 12px;
  }
  .alert-text {
    color: var(--text);
  }
  .files {
    list-style: none;
    margin: 0 0 6px;
    padding: 0;
    max-height: 320px;
    overflow: auto;
  }
  .files li {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) 80px 100px;
    gap: 8px;
    align-items: center;
    padding: 2px 0;
    font-size: 12px;
  }
  .path {
    direction: rtl;
    text-align: left;
  }
</style>
