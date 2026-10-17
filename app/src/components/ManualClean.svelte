<script lang="ts">
  // Reviews and runs a plan of hand-picked files (Disk view, large/old
  // files, duplicates). These are always red tier and always go to the
  // recovery bin; the backend built and checked the exact list.
  import { type Backend, errorMessage } from "../lib/backend.ts";
  import { sizeAndCount } from "../lib/cleaner.ts";
  import { formatCount, formatSize } from "../lib/format.ts";
  import type { CleanReport, ManualPlanView } from "../lib/types.ts";
  import CleanResult from "./CleanResult.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import TierBadge from "./TierBadge.svelte";

  let {
    api,
    request = $bindable(),
    onNavigate,
    onDone,
    onError,
  }: {
    api: Backend;
    /** Set to open the review dialog. */
    request: { title: string; view: ManualPlanView } | null;
    onNavigate: (tab: "recover") => void;
    onDone?: (report: CleanReport) => void;
    onError: (message: string) => void;
  } = $props();

  let running = $state<{ done: number; total: number } | null>(null);
  let result = $state<CleanReport | null>(null);

  async function run() {
    const r = request;
    request = null;
    if (!r) return;
    result = null;
    running = { done: 0, total: 0 };
    try {
      await api.cleanExecute(r.view.plan.planId, (e) => {
        if (e.type === "progress") running = { done: e.done, total: e.total };
        else {
          running = null;
          if (e.type === "done") {
            result = e.report;
            onDone?.(e.report);
          } else onError(e.message);
        }
      });
    } catch (e) {
      running = null;
      onError(errorMessage(e));
    }
  }
</script>

{#if running}
  <div class="notice" aria-live="polite">
    <Icon name="sparkle" size={18} />
    <span>
      Moving files to the recovery bin…
      {#if running.total > 0}{formatCount(running.done)} of {formatCount(running.total)}{/if}
    </span>
    <button class="btn" onclick={() => api.cancelClean()}><Icon name="x" /> Cancel</button>
  </div>
{/if}
{#if result}
  <CleanResult report={result} onRecover={() => onNavigate("recover")} onDismiss={() => (result = null)} />
{/if}

<Modal open={request !== null} title={request?.title ?? ""} wide onClose={() => (request = null)}>
  {#if request}
    {@const p = request.view.plan}
    <p class="tier-note">
      <TierBadge tier="red" />
      <span>
        Files you pick yourself are always <strong>Review</strong> tier: they are moved to the recovery bin, never deleted
        directly, so you can put them back from the Recover tab.
      </span>
    </p>
    {#if p.count > 0}
      <p><strong>{sizeAndCount(p.totalSize, p.count)}</strong> will be moved.</p>
      <details>
        <summary>Largest files</summary>
        <ul class="sample">
          {#each request.view.sample.slice(0, 50) as f (f.path)}
            <li><span class="mono ellipsis" title={f.path}>{f.path}</span> <span class="muted num">{formatSize(f.size)}</span></li>
          {/each}
        </ul>
      </details>
    {:else}
      <p>Nothing here can be cleaned.</p>
    {/if}
    {#if request.view.rejected.length > 0}
      <div>
        <p class="muted">Left out:</p>
        <ul class="sample">
          {#each request.view.rejected as r (r.path)}
            <li><span class="mono ellipsis" title={r.path}>{r.path}</span> <span class="muted">{r.reason}</span></li>
          {/each}
        </ul>
      </div>
    {/if}
    <p class="muted">Protected Windows files, excluded items, links, and cloud files are always left alone.</p>
  {/if}
  {#snippet actions()}
    <button class="btn" onclick={() => (request = null)}>Cancel</button>
    {#if request}
      {@const p = request.view.plan}
      <button class="btn primary" onclick={run} disabled={p.count === 0}>
        Move {sizeAndCount(p.totalSize, p.count)} to the recovery bin
      </button>
    {/if}
  {/snippet}
</Modal>

<style>
  .tier-note {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    margin: 0;
  }
  p {
    margin: 0;
  }
  .sample {
    margin: 4px 0 0;
    padding-left: 18px;
    max-height: 180px;
    overflow: auto;
    font-size: 12px;
  }
  .sample li {
    display: flex;
    gap: 8px;
    justify-content: space-between;
  }
</style>
