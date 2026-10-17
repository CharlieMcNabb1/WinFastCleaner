<script lang="ts">
  import { type Backend, errorMessage } from "../lib/backend.ts";
  import { formatCount, formatDate, formatDateTime, formatPercent, formatSize, percent } from "../lib/format.ts";
  import type { NodeView, SortKey } from "../lib/types.ts";
  import FlagBadges from "./FlagBadges.svelte";
  import Icon from "./Icon.svelte";

  let {
    api,
    scanId,
    nodeId,
    folderSize,
    driveTotal,
    canReveal,
    refreshKey,
    onOpen,
    onNotice,
    onSendToCleaner,
    onExclude,
  }: {
    api: Backend;
    scanId: number;
    nodeId: number;
    folderSize: number;
    driveTotal: number | null;
    canReveal: boolean;
    /** Changes when protected/excluded marks may have changed. */
    refreshKey: number;
    onOpen: (id: number) => void;
    onNotice: (message: string) => void;
    onSendToCleaner: (node: NodeView) => void;
    onExclude: (node: NodeView) => void;
  } = $props();

  const PAGE = 300;
  let sort = $state<SortKey>("size");
  let descending = $state(true);
  let limit = $state(PAGE);
  let items = $state<NodeView[]>([]);
  let total = $state(0);
  let restCount = $state(0);
  let restSize = $state(0);
  let menu = $state<{ x: number; y: number } | null>(null);
  // Kept separately so closing the menu never leaves its items without a
  // target while they are torn down.
  let menuNode = $state<NodeView | null>(null);

  // A new folder starts at the first page.
  let lastFolder = -1;
  $effect.pre(() => {
    if (nodeId !== lastFolder) {
      lastFolder = nodeId;
      limit = PAGE;
    }
  });

  $effect(() => {
    void refreshKey;
    const query = { offset: 0, limit, sort, descending, dirsOnly: false };
    const [scan, id] = [scanId, nodeId];
    api
      .listChildren(scan, id, query)
      .then((page) => {
        if (scan !== scanId || id !== nodeId) return;
        items = page.items;
        total = page.total;
        restCount = page.restCount;
        restSize = page.restSize;
      })
      .catch((e) => onNotice(errorMessage(e)));
  });

  const COLUMNS: { key: SortKey | null; label: string; numeric: boolean }[] = [
    { key: "name", label: "Name", numeric: false },
    { key: "size", label: "Size", numeric: true },
    { key: "allocated", label: "On disk", numeric: true },
    { key: null, label: "% of folder", numeric: true },
    { key: null, label: "% of drive", numeric: true },
    { key: "files", label: "Files", numeric: true },
    { key: "modified", label: "Modified", numeric: true },
  ];

  function setSort(key: SortKey) {
    if (sort === key) descending = !descending;
    else {
      sort = key;
      descending = key !== "name";
    }
  }

  function ariaSort(key: SortKey | null): "ascending" | "descending" | "none" | undefined {
    if (!key) return undefined;
    if (sort !== key) return "none";
    return descending ? "descending" : "ascending";
  }

  function openMenu(ev: MouseEvent, node: NodeView) {
    ev.preventDefault();
    // Keep the menu inside the window.
    const x = Math.max(4, Math.min(ev.clientX, window.innerWidth - 230));
    const y = Math.max(4, Math.min(ev.clientY, window.innerHeight - 200));
    menuNode = node;
    menu = { x, y };
  }

  async function reveal(node: NodeView) {
    menu = null;
    try {
      await api.openInExplorer(scanId, node.id);
    } catch (e) {
      onNotice(errorMessage(e));
    }
  }

  async function copyPath(node: NodeView) {
    menu = null;
    try {
      const detail = await api.getNode(scanId, node.id);
      await navigator.clipboard.writeText(detail.path);
      onNotice("Path copied");
    } catch (e) {
      onNotice(errorMessage(e));
    }
  }

  const PROTECTED_TIP = "Protected: Windows or the app needs this, so the cleaner never touches it.";
  const EXCLUDED_TIP = "On your exclusions list: never cleaned.";

  function onKey(ev: KeyboardEvent) {
    if (ev.key === "Escape") menu = null;
  }
</script>

<svelte:window onkeydown={onKey} onclick={() => (menu = null)} onblur={() => (menu = null)} />

<div class="list">
  <table>
    <caption class="sr-only">Contents of this folder, {formatCount(total)} items</caption>
    <thead>
      <tr>
        {#each COLUMNS as col (col.label)}
          <th class:num={col.numeric} aria-sort={ariaSort(col.key)} scope="col">
            {#if col.key}
              {@const key = col.key}
              <button class="sort" onclick={() => setSort(key)}>
                {col.label}
                {#if sort === key}<span aria-hidden="true">{descending ? "▼" : "▲"}</span>{/if}
              </button>
            {:else}
              {col.label}
            {/if}
          </th>
        {/each}
        <th><span class="sr-only">Actions</span></th>
      </tr>
    </thead>
    <tbody>
      {#each items as node (node.id)}
        <tr oncontextmenu={(e) => openMenu(e, node)} class:dir={node.isDir}>
          <td>
            <div class="name">
            {#if node.isDir}
              <button class="open" onclick={() => onOpen(node.id)} title={`Open ${node.name}`}>
                <span class="ico folder"><Icon name="folder" size={15} /></span>
                <span class="text">{node.name}</span>
              </button>
            {:else}
              <span class="file">
                <span class="ico"><Icon name="file" size={15} /></span>
                <span class="text" title={node.name}>{node.name}</span>
              </span>
            {/if}
            <FlagBadges flags={node.flags} />
            {#if node.protected}
              <span class="mark" title={PROTECTED_TIP}><Icon name="lock" size={13} label={PROTECTED_TIP} /></span>
            {:else if node.excluded}
              <span class="mark" title={EXCLUDED_TIP}><Icon name="ban" size={13} label={EXCLUDED_TIP} /></span>
            {/if}
            </div>
          </td>
          <td class="num">{formatSize(node.size)}</td>
          <td class="num muted">{formatSize(node.allocated)}</td>
          <td class="num pct">
            <span class="bar-track"><span class="bar-fill" style:width={`${percent(node.size, folderSize)}%`}></span></span>
            <span class="pct-text">{formatPercent(node.size, folderSize)}</span>
          </td>
          <td class="num muted">{driveTotal ? formatPercent(node.size, driveTotal) : "—"}</td>
          <td class="num muted">{node.isDir ? formatCount(node.files) : ""}</td>
          <td class="num muted" title={formatDateTime(node.modified)}>{formatDate(node.modified)}</td>
          <td class="actions">
            <button
              class="btn ghost icon-btn"
              aria-label={`Actions for ${node.name}`}
              onclick={(e) => {
                e.stopPropagation();
                openMenu(e, node);
              }}
            >
              <Icon name="more" />
            </button>
          </td>
        </tr>
      {/each}
      {#if restCount > 0}
        <tr class="rest">
          <td colspan="8">
            <button class="btn" onclick={() => (limit += PAGE)}>
              Show more ({formatCount(restCount)} more items, {formatSize(restSize)})
            </button>
          </td>
        </tr>
      {/if}
      {#if items.length === 0}
        <tr><td colspan="8" class="muted empty">This folder is empty.</td></tr>
      {/if}
    </tbody>
  </table>
</div>

{#if menu && menuNode}
  {@const target = menuNode}
  <div class="menu" role="menu" style:left={`${menu.x}px`} style:top={`${menu.y}px`}>
    {#if target.isDir}
      <button role="menuitem" onclick={() => onOpen(target.id)}><Icon name="folderOpen" /> Open</button>
    {/if}
    <button role="menuitem" disabled={!canReveal} onclick={() => reveal(target)} title={canReveal ? "" : "Only available on Windows"}>
      <Icon name="external" /> Show in File Explorer
    </button>
    <button role="menuitem" onclick={() => copyPath(target)}><Icon name="copy" /> Copy path</button>
    <hr />
    <button
      role="menuitem"
      disabled={target.protected || target.excluded}
      title={target.protected ? PROTECTED_TIP : target.excluded ? EXCLUDED_TIP : "Review and move it to the recovery bin"}
      onclick={() => {
        menu = null;
        onSendToCleaner(target);
      }}
    >
      <Icon name="sparkle" /> Send to cleaner…
    </button>
    <button
      role="menuitem"
      disabled={target.excluded}
      onclick={() => {
        menu = null;
        onExclude(target);
      }}
    >
      <Icon name="ban" /> {target.excluded ? "Already excluded" : "Add to exclusions"}
    </button>
  </div>
{/if}

<style>
  .list {
    overflow: auto;
    height: 100%;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  table {
    width: 100%;
    /* Below this the Name column would be squeezed; scroll instead. */
    min-width: 820px;
    border-collapse: collapse;
    table-layout: fixed;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    font-weight: 600;
    font-size: 12px;
    color: var(--text-2);
    text-align: left;
    padding: 6px 8px;
    z-index: 1;
  }
  /* Name takes whatever is left. */
  th:nth-child(2),
  th:nth-child(3) {
    width: 88px;
  }
  th:nth-child(4) {
    width: 124px;
  }
  th:nth-child(5) {
    width: 84px;
  }
  th:nth-child(6) {
    width: 76px;
  }
  th:nth-child(7) {
    width: 116px;
  }
  th:last-child {
    width: 40px;
  }
  th.num {
    text-align: right;
  }
  .sort {
    border: 0;
    background: none;
    padding: 0;
    font-weight: 600;
    color: inherit;
  }
  td {
    padding: 4px 8px;
    border-bottom: 1px solid var(--surface-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  tr:hover td {
    background: var(--surface-2);
  }
  .name {
    display: flex;
    align-items: center;
    min-width: 0;
  }
  .open,
  .file {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    border: 0;
    background: none;
    padding: 0;
    text-align: left;
  }
  .open .text {
    font-weight: 500;
  }
  .open:hover .text {
    text-decoration: underline;
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ico {
    display: inline-flex;
    color: var(--text-2);
  }
  .ico.folder {
    color: var(--accent);
  }
  .pct {
    display: flex;
    align-items: center;
    gap: 6px;
    justify-content: flex-end;
  }
  .pct .bar-track {
    width: 48px;
    flex: none;
  }
  .pct-text {
    min-width: 44px;
  }
  .actions {
    text-align: right;
    padding: 0 4px;
  }
  .icon-btn {
    padding: 2px 6px;
  }
  .rest td,
  .empty {
    text-align: center;
    padding: 10px;
  }
  .menu {
    position: fixed;
    z-index: 10;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: 0 8px 24px rgb(0 0 0 / 18%);
  }
  .menu button {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border: 0;
    border-radius: 6px;
    background: none;
    text-align: left;
  }
  .menu button:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .menu hr {
    width: 100%;
    margin: 4px 0;
    border: 0;
    border-top: 1px solid var(--border);
  }
  .mark {
    display: inline-flex;
    margin-left: 4px;
    color: var(--text-2);
    vertical-align: -2px;
  }
</style>
