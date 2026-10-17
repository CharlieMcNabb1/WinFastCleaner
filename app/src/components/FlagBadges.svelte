<script lang="ts">
  import { Flags } from "../lib/types.ts";
  import Icon, { type IconName } from "./Icon.svelte";

  let { flags }: { flags: number } = $props();

  const BADGES: { bit: number; icon: IconName; label: string }[] = [
    { bit: Flags.LINK, icon: "link", label: "Link (symlink or junction). Not followed, so its target isn't counted here." },
    { bit: Flags.HARDLINK_DUP, icon: "layers", label: "Hard link. This file's size is counted once, at another of its names." },
    {
      bit: Flags.HAS_HARDLINKS,
      icon: "layers",
      label: "Contains hard-linked files that are counted once. File Explorer counts every name, so it may show a larger size.",
    },
    { bit: Flags.UNREADABLE, icon: "alert", label: "This folder couldn't be read (access denied or in use)." },
    { bit: Flags.HAS_UNREADABLE, icon: "alert", label: "Some folders inside couldn't be read, so the size may be incomplete." },
    { bit: Flags.CLOUD, icon: "cloud", label: "Cloud file. Its contents aren't stored on this disk." },
    { bit: Flags.METAFILE, icon: "database", label: "NTFS file system metadata." },
  ];

  let shown = $derived(
    BADGES.filter((b) => flags & b.bit).filter(
      // "Unreadable" already implies "has unreadable"; avoid two alerts.
      (b) => !(b.bit === Flags.HAS_UNREADABLE && flags & Flags.UNREADABLE),
    ),
  );
</script>

{#each shown as b (b.bit)}
  <span class="badge" title={b.label}><Icon name={b.icon} size={13} label={b.label} /></span>
{/each}

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    margin-left: 4px;
    color: var(--text-2);
    vertical-align: -2px;
  }
</style>
