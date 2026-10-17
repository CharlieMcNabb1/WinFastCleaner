<script lang="ts">
  import { STATUS } from "../lib/system.ts";
  import type { HealthStatus } from "../lib/types.ts";
  import Icon from "./Icon.svelte";

  let { status }: { status: HealthStatus } = $props();
  let s = $derived(STATUS[status]);
</script>

<!-- Color is always paired with an icon and a label. -->
<span class="status {status}">
  <Icon name={s.icon} size={12} />
  {s.label}
</span>

<style>
  .status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px 8px 1px 6px;
    border-radius: 999px;
    border: 1px solid currentColor;
    font-size: 12px;
    font-weight: 600;
    line-height: 18px;
    white-space: nowrap;
  }
  .green {
    color: var(--tier-green);
    background: var(--tier-green-bg);
  }
  .yellow {
    color: var(--tier-yellow);
    background: var(--tier-yellow-bg);
  }
  .red {
    color: var(--tier-red);
    background: var(--tier-red-bg);
  }
  .unknown {
    color: var(--text-2);
    background: var(--surface-2);
  }
</style>
