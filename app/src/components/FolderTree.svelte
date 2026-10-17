<script lang="ts">
  import type { Backend } from "../lib/backend.ts";
  import type { NodeView } from "../lib/types.ts";
  import TreeItem from "./TreeItem.svelte";

  let {
    api,
    scanId,
    root,
    currentId,
    lineage,
    onOpen,
  }: {
    api: Backend;
    scanId: number;
    root: NodeView;
    currentId: number;
    lineage: ReadonlySet<number>;
    onOpen: (id: number) => void;
  } = $props();
</script>

<ul class="tree" role="tree" aria-label="Folders">
  {#key scanId}
    <TreeItem {api} {scanId} node={root} parentSize={root.size} depth={0} {currentId} {lineage} {onOpen} />
  {/key}
</ul>

<style>
  .tree {
    list-style: none;
    margin: 0;
    padding: 4px;
  }
</style>
