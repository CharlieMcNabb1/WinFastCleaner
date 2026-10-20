# Design system: "Ledger"

A calm, roomy ledger for a fast, trustworthy Windows utility. Warm neutrals,
large figures, and every list reads as a sized account of where the space went.

The tokens live in `app/src/app.css` as CSS variables. `app/src/lib/tokens.test.ts`
checks their contrast and keeps the two dark blocks in sync.

## Color rules

- **Green, yellow, and red are only for the safety tiers** (Safe, Optional,
  Review). A tier color is always shown with its icon and label, never alone.
- **Petrol** (`--accent`) is for primary actions, selection, and focus only.
- Text pairs are at least **4.5:1** in both themes.
- Light, dark, and "Same as Windows" all use the same token names. Only the
  values change.

## Color tokens

| Token | Light | Dark | Use |
|---|---|---|---|
| `--bg` | `#f7f6f3` | `#121110` | Window background |
| `--surface` | `#ffffff` | `#1a1917` | Panels, sidebar |
| `--surface-2` | `#f1efea` | `#23221f` | Hover, nested areas |
| `--surface-3` | `#e8e5de` | `#2b2a26` | Pressed, strong fill |
| `--border` | `#e2dfd8` | `#2f2d29` | Hairlines |
| `--text` | `#1c1b19` | `#eceae5` | Body text |
| `--text-2` | `#5e5a52` | `#a39e94` | Secondary text |
| `--accent` | `#0e7490` | `#4fb3cb` | Primary buttons, links, active tab |
| `--accent-2` | `#e4f1f4` | `#12313a` | Selected background |
| `--accent-text` | `#ffffff` | `#06191f` | Text on `--accent` |
| `--focus`, `--bar` | `#0e7490` | `#4fb3cb` | Focus ring, usage bars |
| `--bar-track` | `#ece9e3` | `#2f2d29` | Empty part of a bar |
| `--notice-bg` / `-border` | `#eaf4f6` / `#b9dae2` | `#142a30` / `#24505b` | Info banners |
| `--alert-bg` / `-border` | `#fbf3e4` / `#ebd3a6` | `#2a2213` / `#5a4622` | Error banners |
| `--tier-green` / `-bg` | `#24753a` / `#e8f4ea` | `#6bcb82` / `#132419` | Safe |
| `--tier-yellow` / `-bg` | `#8f5600` / `#fbf0dc` | `#e9b25e` / `#2b2213` | Optional |
| `--tier-red` / `-bg` | `#a8261b` / `#fbeae7` | `#ef8a7e` / `#2e1816` | Review |

## Scales

- **Space:** `--space-1` to `--space-7` = 4, 8, 12, 16, 24, 32, 48px. Rows are
  52px; views have 24px padding.
- **Type:** `--text-xs` to `--text-xl` = 12, 14, 16, 22, 28px. Body is 14px,
  h1 is 22/600, and hero figures (junk total, growth) are 28/600. Sizes and
  counts use `tabular-nums`.
- **Radius:** `--radius-sm` 6px (inputs, badges), `--radius` 8px (buttons),
  `--radius-lg` 12px (panels). Panels get the one soft `--shadow`.
- **Fonts:** IBM Plex Sans and IBM Plex Mono, bundled locally so the app works
  offline. Until then, `--font` and `--mono` stay Segoe UI Variable and
  Cascadia Mono.

## Signature: size bars

Every size column has an inline proportional bar in a neutral color, never a
tier color. The Cleaner, the Disk table, What grew, and Files then all scan
the same way.

Cleaner tier rows: each group is one panel with its total in the header. A
rule row has the checkbox, name, size bar, size, and expand chevron on the
first line, and the tier chip and description on the second line.

```
┌ Windows ───────────────────────────────────────── 4.8 GB ┐
│ ☑  Temporary files                ▓▓▓▓░░░░░░  1.2 GB   › │
│    [✓ Safe]  Rebuilt on demand              3,104 files  │
│    What you lose: nothing                                │
└──────────────────────────────────────────────────────────┘
```

## Not done yet (restyle)

The tokens are in place, but the screens haven't been restyled yet. Still to do:

- Apply the scales, size bars, and tier row layout to the screens.
- Bundle IBM Plex and switch `--font` and `--mono` to it.
- Retune the treemap palette (`Treemap.svelte`) around petrol, and replace its
  hardcoded stroke and fill colors (which still use the old background) with
  theme values.
- `.btn.danger.primary` puts white text on `--tier-red`, which is below 4.5:1
  in dark mode.
