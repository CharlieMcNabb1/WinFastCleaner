<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    open,
    title,
    onClose,
    children,
    actions,
    wide = false,
    dismissable = true,
  }: {
    open: boolean;
    title: string;
    onClose: () => void;
    children: Snippet;
    actions: Snippet;
    wide?: boolean;
    /** False while something is running that can't be walked away from. */
    dismissable?: boolean;
  } = $props();

  let dialog = $state<HTMLDialogElement | null>(null);
  const titleId = `modal-${Math.random().toString(36).slice(2)}`;

  // The native <dialog> traps focus and restores it on close.
  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });

  function onCancel(e: Event) {
    if (!dismissable) e.preventDefault();
  }
</script>

<dialog bind:this={dialog} class:wide aria-labelledby={titleId} onclose={onClose} oncancel={onCancel}>
  {#if open}
    <h2 id={titleId}>{title}</h2>
    <div class="body">{@render children()}</div>
    <div class="actions">{@render actions()}</div>
  {/if}
</dialog>

<style>
  dialog {
    width: min(520px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    padding: 18px 20px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 16px 48px rgb(0 0 0 / 28%);
  }
  dialog.wide {
    width: min(760px, calc(100vw - 32px));
  }
  dialog::backdrop {
    background: rgb(10 12 20 / 45%);
  }
  h2 {
    margin: 0 0 10px;
    font-size: 17px;
    font-weight: 600;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 18px;
    flex-wrap: wrap;
  }
</style>
