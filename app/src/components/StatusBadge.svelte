<script lang="ts">
  import { STATUS } from "../lib/system.ts";
  import type { HealthStatus } from "../lib/types.ts";
  import Icon from "./Icon.svelte";

  let { status }: { status: HealthStatus } = $props();
  let s = $derived(STATUS[status]);
</script>

<!-- Neutral on purpose: green/yellow/red belong to the safety tiers
     (docs/design.md). Severity shows as emphasis, plus the icon and label. -->
<span class="status {status}">
  <Icon name={s.icon} size={12} />
  {s.label}
</span>

<style>
  .status {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 1px var(--space-2) 1px 6px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    font-size: var(--text-xs);
    font-weight: 600;
    line-height: 18px;
    white-space: nowrap;
  }
  .green {
    color: var(--text-2);
    background: var(--surface);
  }
  .yellow {
    color: var(--text);
    background: var(--surface-3);
  }
  .red {
    color: var(--surface);
    background: var(--text);
    border-color: var(--text);
  }
  .unknown {
    color: var(--text-2);
    background: transparent;
    border-style: dashed;
  }
</style>
