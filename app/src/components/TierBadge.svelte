<script lang="ts">
  import { TIERS } from "../lib/cleaner.ts";
  import type { Tier } from "../lib/types.ts";
  import Icon from "./Icon.svelte";

  let { tier }: { tier: Tier } = $props();
  let t = $derived(TIERS[tier]);
</script>

<!-- Color is always paired with an icon and a label. -->
<span class="tier {tier}" title={t.meaning}>
  <Icon name={t.icon} size={12} />
  {t.label}
</span>

<style>
  .tier {
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
</style>
