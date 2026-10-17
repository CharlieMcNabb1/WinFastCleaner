<script lang="ts">
  import type { Backend } from "../lib/backend.ts";
  import { formatSize, percent } from "../lib/format.ts";
  import type { NodeView } from "../lib/types.ts";
  import FlagBadges from "./FlagBadges.svelte";
  import Icon from "./Icon.svelte";
  import TreeItem from "./TreeItem.svelte";

  let {
    api,
    scanId,
    node,
    parentSize,
    depth,
    currentId,
    lineage,
    onOpen,
  }: {
    api: Backend;
    scanId: number;
    node: NodeView;
    parentSize: number;
    depth: number;
    currentId: number;
    lineage: ReadonlySet<number>;
    onOpen: (id: number) => void;
  } = $props();

  const PAGE = 200;
  let open = $state(false);
  let children = $state<NodeView[] | null>(null);
  let more = $state(0);
  let loading = false;

  // Expand the root, and every folder on the path to the one being viewed.
  $effect(() => {
    if (lineage.has(node.id) && (node.id !== currentId || depth === 0)) open = true;
  });

  $effect(() => {
    if (open && children === null && !loading) {
      loading = true;
      api
        .listChildren(scanId, node.id, { offset: 0, limit: PAGE, sort: "size", descending: true, dirsOnly: true })
        .then((page) => {
          children = page.items;
          more = page.restCount;
        })
        .catch(() => {
          children = [];
        })
        .finally(() => {
          loading = false;
        });
    }
  });

  let expandable = $derived(node.dirs > 0);
</script>

<li role="treeitem" aria-expanded={expandable ? open : undefined} aria-selected={node.id === currentId}>
  <div class="row" class:current={node.id === currentId} style:padding-left={`${depth * 14 + 4}px`}>
    <button
      class="twisty"
      tabindex="-1"
      aria-hidden="true"
      disabled={!expandable}
      onclick={() => (open = !open)}
    >
      {#if expandable}<Icon name={open ? "chevronDown" : "chevronRight"} size={14} />{/if}
    </button>
    <button class="label" onclick={() => onOpen(node.id)} title={node.name}>
      <span class="folder-icon"><Icon name={open ? "folderOpen" : "folder"} size={15} /></span>
      <span class="name">{node.name}</span>
      <FlagBadges flags={node.flags} />
      <span class="size num muted">{formatSize(node.size)}</span>
    </button>
  </div>
  <div class="share" style:margin-left={`${depth * 14 + 26}px`}>
    <span style:width={`${percent(node.size, parentSize)}%`}></span>
  </div>
  {#if open && children}
    <ul role="group">
      {#each children as child (child.id)}
        <TreeItem {api} {scanId} node={child} parentSize={node.size} depth={depth + 1} {currentId} {lineage} {onOpen} />
      {/each}
      {#if more > 0}
        <li class="more muted" style:padding-left={`${(depth + 1) * 14 + 26}px`}>
          {more} smaller folders not shown
        </li>
      {/if}
    </ul>
  {/if}
</li>

<style>
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: center;
    border-radius: 6px;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.current {
    background: var(--accent-2);
  }
  .twisty {
    width: 20px;
    height: 24px;
    border: 0;
    background: none;
    color: var(--text-2);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
  }
  .label {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    border: 0;
    background: none;
    padding: 3px 6px 3px 0;
    text-align: left;
  }
  .folder-icon {
    color: var(--accent);
    display: inline-flex;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .size {
    margin-left: auto;
    padding-left: 8px;
    font-size: 12px;
  }
  .share {
    height: 2px;
    margin-right: 8px;
    margin-bottom: 1px;
    background: transparent;
  }
  .share span {
    display: block;
    height: 100%;
    background: var(--bar);
    opacity: 0.45;
    border-radius: 1px;
  }
  .more {
    font-size: 12px;
    padding: 2px 0 4px;
  }
</style>
