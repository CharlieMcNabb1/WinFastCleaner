// The app's color theme: follows Windows by default, or the user's choice
// in Settings. Components read `isDark()` (for canvas drawing); CSS reads
// the `data-theme` attribute.

import type { Theme } from "./types.ts";

const media = typeof window !== "undefined" ? window.matchMedia("(prefers-color-scheme: dark)") : null;

const theme = $state<{ choice: Theme; systemDark: boolean }>({
  choice: "system",
  systemDark: media?.matches ?? false,
});

media?.addEventListener("change", (e) => (theme.systemDark = e.matches));

export function isDark(): boolean {
  return theme.choice === "dark" || (theme.choice === "system" && theme.systemDark);
}

export function applyTheme(choice: Theme): void {
  theme.choice = choice;
  if (choice === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = choice;
}
