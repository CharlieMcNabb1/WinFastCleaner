# Design system: "Graphite"

A quiet, monochrome Windows 11 utility that anyone can use. Five greys, from
paper white to near-black, carry the whole interface. The safety tiers' green,
yellow, and red are the only colors on screen. Each screen opens with a
plain-language summary and one clear action. Every detail and advanced tool is
still there, one expand or click away. The finish (greys, soft corners, calm
motion) should feel like Apple. The structure (navigation pane, settings
expanders, switches, dialog order) is Windows 11.

The tokens live in `app/src/app.css` as CSS variables. `app/src/lib/tokens.test.ts`
checks their contrast and keeps the two dark blocks in sync.

## Principles

1. **One primary action per screen.** A view shows at most one primary
   (`.btn.primary`) button at a time. Dialogs have their own. Everything else is
   a default `.btn`, a `.btn.ghost`, or a link. Some views have no primary
   action (What grew, Startup, Settings).
2. **Summary first.** A view opens with its answer: a hero figure or one status
   sentence. A collapsed row shows name, size, and tier, and nothing else.
3. **Details are one step away and never removed.** File lists, technical
   reasons, paths, sizes on disk, hard-link and admin flags, IDs, versions, and
   command lines live in expanded rows, tooltips, or an Advanced section.
   Nothing that exists today is deleted.
4. **Safety facts are never hidden.** These stay visible where the user
   decides (on the row and in the confirm dialog):
   - tier chips
   - "What you lose"
   - whether files go to the recovery bin or can't be restored
   - notices about admin rights or skipped files

   Disclosure is for detail, never for consequences.
5. **Plain language by default.** Technical terms appear only in expanded
   content and tooltips (see the glossary below).
6. **Windows 11 structure, Apple finish.** Follow Windows conventions for
   navigation, controls, dialogs, and type. Palette, corners, and motion carry
   the polish.

## Palette

| Name | Value | Notes |
|---|---|---|
| Paper | `#fafaf8` | Light background, dark text |
| Mist | `#dadada` | Light fills, hairlines, tracks |
| Silver | `#b0b3b8` | Dark secondary text and bars. **Never text on light**: it is 2.0:1 on Paper |
| Slate | `#43464b` | Light secondary text and bars, dark hairlines and selection |
| Ink | `#0f1012` | Light text and primary, dark background |

Derived tints (marked D below) stay on the same grey axis. Light ones are
white, or a mix of Paper and Mist. Dark ones are straight mixes of Ink and
Slate (20%, 35%, 50%) or of Slate and Silver (40%).

## Color rules

- **Green, yellow, and red are only for the safety tiers** (Safe, Optional,
  Review), and they are the only chroma in the UI. A tier color is always shown
  with its icon and label, never alone.
- **`--accent`** (Ink in light, Paper in dark) is only for the primary action,
  selection, focus, and links. The accent is the same as the text color, so
  links are always underlined.
- Native checkboxes, radios, and switches use `accent-color: var(--accent)`,
  so the system blue never shows.
- **Health status (Home) is not a tier.** `StatusBadge` is neutral:
  OK = `--text-2` on `--surface`, Attention = `--text` on `--surface-3`,
  Problem = `--surface` on `--text`, Unknown = a dashed `--border`. It always
  has an icon and a label.
- **Notices and alerts are both grey.** An alert uses the alert icon,
  `role="alert"`, and the stronger `--alert-border`.
- **Destructive actions** (Delete, Empty bin) are a default `.btn` with a
  specific verb and size ("Delete 2.1 GB"), followed by a confirm dialog. They
  are never tier-colored.
- **Contrast:**
  - Text is at least **4.5:1**.
  - The focus ring, bars, control outlines (`--border-strong`), and alert
    borders are at least **3:1**.
  - `--border` hairlines are decorative and exempt.
- Light, dark, and "Same as Windows" all use the same token names. Only the
  values change.

## Color tokens

| Token | Light | Dark | Use |
|---|---|---|---|
| `--bg` | `#fafaf8` Paper | `#0f1012` Ink | Window background |
| `--surface` | `#ffffff` D | `#191b1d` D | Panels, sidebar, dialogs, menus |
| `--surface-2` | `#f0f0ef` D | `#212326` D | Hover, nested areas |
| `--surface-3` | `#dadada` Mist | `#292b2e` D | Pressed, strong fill, Attention badge |
| `--border` | `#dadada` Mist | `#43464b` Slate | Decorative hairlines |
| `--border-strong` | `#7e8186` D | `#6f7277` D | Outlines of inputs, checkboxes, switches |
| `--text` | `#0f1012` Ink | `#fafaf8` Paper | Body text |
| `--text-2` | `#43464b` Slate | `#b0b3b8` Silver | Secondary text |
| `--accent` | `#0f1012` Ink | `#fafaf8` Paper | Primary button, links, active tab |
| `--accent-2` | `#dadada` Mist | `#43464b` Slate | Selected background |
| `--accent-text` | `#fafaf8` Paper | `#0f1012` Ink | Text on `--accent` |
| `--focus` | `#0f1012` Ink | `#fafaf8` Paper | Focus ring |
| `--bar` | `#43464b` Slate | `#b0b3b8` Silver | Size bars, usage meters, progress |
| `--bar-track` | `#dadada` Mist | `#43464b` Slate | Empty part of a bar |
| `--notice-bg` / `-border` | `#f0f0ef` / `#dadada` | `#212326` / `#43464b` | Info banners |
| `--alert-bg` / `-border` | `#ffffff` / `#43464b` | `#191b1d` / `#b0b3b8` | Error banners |
| `--shadow` | Ink at 6% | black at 40% | Raised things only |
| `--tier-green` / `-bg` | `#24753a` / `#e8f4ea` | `#6bcb82` / `#132419` | Safe |
| `--tier-yellow` / `-bg` | `#8f5600` / `#fbf0dc` | `#e9b25e` / `#2b2213` | Optional |
| `--tier-red` / `-bg` | `#a8261b` / `#fbeae7` | `#ef8a7e` / `#2e1816` | Review |

## Type

- **Font:** Segoe UI Variable, the Windows 11 system font. Nothing is bundled,
  and on Windows 10 it falls back to Segoe UI. `--font` already uses it.
  `--mono` (Cascadia Mono) is only for paths, IDs, versions, and commands, and
  only in expanded detail.
- **Scale:** `--text-xs` to `--text-xl` = 12, 14, 16, 22, 28px. Body is 14px,
  h1 is 22/600, panel titles are 16/600, and hero figures (junk total, growth,
  bin size) are 28/600. Sizes and counts use `tabular-nums`. Only weights 400
  and 600 are used.
- Use sentence case everywhere. Buttons are a verb, plus the size when there
  is one: "Clean 1.2 GB", "Update 3 apps".

## Space and layout

- **Space:** `--space-1` to `--space-7` = 4, 8, 12, 16, 24, 32, 48px.
- **View padding:** `--space-5` top and bottom, `--space-6` left and right.
  Sections are `--space-5` apart. Panels have `--space-4` padding and a
  `--space-3` gap.
- **Width:** single-column views cap their content at 960px, left-aligned like
  Windows Settings. Disk and Files use the full width for their tables.
- **Rows:** 52px. Detail rows inside an expanded row are at least 40px.
- **View header:** h1 plus one plain summary line in `--text-2` (about 90
  characters at most). Secondary actions such as Refresh or Scan again are
  `.btn.ghost` buttons with an icon, on the right. Longer explanations move to
  a "How this works" expander or into the confirm dialog.
- **Primary action:** if it acts on a selection, it goes in the bottom action
  bar (selection summary on the left, primary button on the right). The bar
  floats `--space-4` in from the view edges, with `--radius-lg` and `--shadow`.
  Otherwise the primary action sits next to the hero figure.

```
Cleaner                                            ⟳ Scan again
4.8 GB of junk found
[notice only when needed]
╭ Windows ─────────────────────────── 4.8 GB ╮
│ rows…                                      │
╰────────────────────────────────────────────╯
Advanced                                                     ⌄
Deep clean, rule problems
 ╭──────────────────────────────────────────────────────────╮
 │ Selected: 1.2 GB (3,104 files)          [ Clean 1.2 GB ] │
 ╰──────────────────────────────────────────────────────────╯
```

## Surfaces and borders

- **Panels:** `--surface` on `--bg` with a 1px `--border` hairline. Panels have
  no shadow and are never nested.
- **Rows inside panels:** no borders. Rows sit `--space-1` in from the panel
  edge. Hover is `--surface-2`, pressed is `--surface-3`, and selected is
  `--accent-2`.
- **Dividers** (1px `--border`) appear only in tables and expanded file lists.
- **`--shadow`** is only for raised things: dialogs, menus, toasts, the
  floating action bar, and the resting edge of buttons.

## Corners

There are no sharp corners. Every surface, control, focus ring, and menu takes
its radius from this scale:

| Token | Value | Use |
|---|---|---|
| `--radius-sm` | 6px | Small controls: in-row icon buttons, tags ("admin", "All users"), treemap blocks |
| `--radius` | 8px | Buttons, inputs, selects, nav items, row hover and selection fills, menu items |
| `--radius-lg` | 12px | Panels, banners, dialogs, menus, toasts, the action bar |
| `--radius-pill` | 999px | Chips only: tier chips, status badges, filter chips |

- **Concentric:** an inner radius equals the outer radius minus the gap. For
  example, a 12px menu with 4px padding has 8px items, and a 12px panel with
  rows inset 4px has 8px row fills.
- **Bars and switch tracks** are rounded to half their own height.
- **Focus ring:** `outline: 2px solid var(--focus); outline-offset: 2px`. In
  Chromium the outline follows the element's `border-radius`, so rings match
  their control.
- **Squircle, progressive enhancement:**
  `@supports (corner-shape: squircle) { … }` adds `corner-shape: squircle` to
  panels, dialogs, menus, and buttons. Chromium has supported `corner-shape`
  since version 139, and WebView2 is evergreen. Anywhere it is unsupported,
  plain `border-radius` applies. Check borders and focus rings in render
  review.

## Disclosure

- **Expander row** (Windows 11 settings expander): a full-width header at
  least 52px tall, with a title, a one-line plain summary, and a chevron at the
  far right that points down and rotates up when open. The whole header is the
  toggle and carries `aria-expanded`. Use native `<details>`/`<summary>` where
  it fits.
- **Advanced:** one expander at the bottom of a view, labeled "Advanced", whose
  summary names what is inside. It is closed by default. It holds power tools
  and never safety notices.
- **Tooltips:** a native `title` for short technical asides. A tooltip is never
  the only place for something: the same information is also in the expanded
  row or in `aria-describedby`, so keyboard users can reach it.
- **Sub-tabs** (Files): an underline selector in `--accent`.

## Motion

Motion should be calm and quick, and Windows-appropriate.

**Tokens:**

| Token | Value | Use |
|---|---|---|
| `--dur-fast` | 120ms | Hover, press, exits |
| `--dur-base` | 200ms | Expanders, view entry, toasts, fades |
| `--dur-slow` | 320ms | Dialog entry, bars filling |
| `--stagger` | 20ms | Delay between list rows |
| `--ease-out` | `cubic-bezier(0.2, 0, 0, 1)` | Entrances and state changes |
| `--ease-in` | `cubic-bezier(0.4, 0, 1, 1)` | Exits |
| `--ease-spring` | see below | Scale and translate on entry only |

```css
--ease-spring: linear(0, 0.039 3%, 0.135 6%, 0.261 9%, 0.398 12%, 0.531 15%,
  0.653 18%, 0.759 21%, 0.846 24%, 0.965 30%, 1.025 36%, 1.046 45%,
  1.032 54%, 1.02 60%, 1.003 72%, 0.998 84%, 1);
```

The spring is damped (ζ ≈ 0.7) with about 4.5% overshoot. Use it only on
transforms, never on opacity or height.

**What animates:**

| What | How |
|---|---|
| Expanders and Advanced | Content `block-size` 0 → auto plus opacity 0 → 1, over `--dur-base` with `--ease-out`. The chevron rotates 180° on the same timing. Closing uses `--dur-fast` with `--ease-in`. This uses `interpolate-size: allow-keywords` on `:root` and `details::details-content`. If those are unsupported, the expander opens instantly. |
| Tab switch | The entering pane goes from opacity 0 and `translateY(4px)` to its final state, over `--dur-base` with `--ease-out`, via a keyframe on `.pane:not([hidden])` that replays each time the pane is shown. The outgoing pane hides instantly. The View Transitions API was considered and not used: it needs a JS wrapper around the tab change, and its snapshot overlay swallows clicks while it runs. |
| Dialogs | **In:** opacity 0 → 1 over `--dur-base` with `--ease-out`, plus `scale(0.96)` → 1 over `--dur-slow` with `--ease-spring`. The backdrop fades over `--dur-base`. Uses `@starting-style` on `dialog[open]`. **Out:** opacity → 0 and `scale(0.98)` over `--dur-fast` with `--ease-in`, with `transition-behavior: allow-discrete` on `display` and `overlay`. |
| Button press | `:active` is `scale(0.97)` over `--dur-fast`. A primary button's hover is `color-mix(in srgb, var(--accent) 88%, var(--surface))`, and its pressed state is the same mix at 80%. |
| Hover and selection | Only background and text color change, over `--dur-fast` with `--ease-out`. Nothing moves on hover. |
| Rows after a scan | The first time results render, rows go from opacity 0 and `translateY(4px)` to their final state, over `--dur-base` with `--ease-out`. The first 12 rows are staggered with `animation-delay: calc(var(--i) * var(--stagger))`, where `--i` is set inline. Later rows appear at once. This does not run on sort, filter, expand, or refresh. |
| Bars | When finished results are first revealed, the fill goes from `scaleX(0)` to 1 from the left, over `--dur-slow` with `--ease-out`, delayed with its row. Progress bars use a `width` transition over `--dur-base`. The indeterminate slide stays as it is. |
| Toasts | Short confirmations ("Added to exclusions") use `role="status"`. **In:** opacity plus `translateY(8px)` → 0, over `--dur-base` with `--ease-spring`. **Out:** opacity over `--dur-fast` with `--ease-in`. |
| Numbers | **Never tween or count up.** A size or count appears in its final form. Live counters during a scan update in place, because a tweened figure would show values that were never measured. |

**Rules:**

- **CSS only, no new dependency.** The only markup involved is the inline
  `--i` on rows.
- **Animate only `transform` and `opacity`.** There are three exceptions:
  expander `block-size`, hover colors, and progress `width`.
- **Nothing blocks input.**
  - No `pointer-events: none` during motion.
  - Nothing waits for `animationend`.
  - A dialog takes input from its first frame.
  - Exits never delay the state change behind them.
- **Reduced motion:** under `prefers-reduced-motion: reduce`, all `--dur-*`
  tokens and `--stagger` become 0ms, so everything is instant. The
  indeterminate progress bar is static (this already exists).

## Controls (Windows 11)

- **Buttons:**
  - Primary is an `--accent` fill with `--accent-text`.
  - Default is a `--surface` fill with a `--border` hairline.
  - Ghost has no fill until hover.
  - All buttons are at least 32px tall, with the icon before the label.
- **Switches and checkboxes:** settings that apply at once use a toggle switch
  (as Startup does). Checkboxes are only for picking items to act on. An
  unchecked control is outlined in `--border-strong`. When on or checked, it
  uses `--accent`.
- **Dialogs:** a title, then a plain summary of what will happen, including its
  safety facts. Buttons sit at the bottom right, **primary first and Cancel
  last** (Windows order). Esc cancels.
- **Navigation pane:** on the left, with icons and labels. The active tab is
  `--accent` text on `--accent-2`, weight 600. Settings is pinned to the bottom
  of the pane.
- **Admin:** "Restart as administrator" has the shield icon and sits inside a
  `.notice`.

## States

| State | Pattern |
|---|---|
| Loading | A panel with a one-line label, `.progress-track` (indeterminate when the total is unknown), and Cancel where the core supports it. The layout doesn't shift. |
| Empty | One plain sentence and the next action as a button ("Scan to find junk files." + Scan). |
| Error | `.alert` with the specific message and a recovery step (Try again, or a link to the right tab). |
| Partial | Show what was found. A one-line link such as "12 items couldn't be read" opens the list. |
| Success | A result panel with what was done, what was skipped, and the undo path ("Restore from Recover"). |
| Permission | `.notice` that says what is skipped and why, with "Restart as administrator". Disabled controls get a tooltip with the reason. |
| Disabled | Controls are dimmed to 0.55 opacity, and the reason is in a tooltip or an adjacent line. |

## Plain language

| Technical | Default label (technical term goes in a tooltip or expanded row) |
|---|---|
| MFT scan / directory walk | Fast scan / Standard scan |
| Allocated size | "On disk", shown only in expanded detail |
| Hard-linked files | Flag icon. Tooltip: "Shared files are counted once" |
| winget, package ID | Windows Package Manager. The ID goes in expanded detail |
| DISM component store cleanup | Deep clean ("Windows component cleanup") |
| Reparse points, junctions | Never shown by default. Called "links" in detail |
| `requires_admin` | Needs administrator |
| Rule ID, rule file | Shown only in expanded rule detail and Advanced |

## Signature: size bars

Every size column has an inline proportional bar in `--bar`. It is never a tier
color. The Cleaner, the Disk table, What grew, and Files all scan the same way.

Cleaner tier rows: each group is one panel with its total in the header. A
collapsed rule row shows the checkbox, name, size bar, size, and expand
chevron on the first line, and the tier chip and "What you lose" on the second
line. The description, file count, admin need, hidden or excluded counts, and
the exact files are one expand away.

```
╭ Windows ───────────────────────────────────────── 4.8 GB ╮
│ ☑  Temporary files                ▓▓▓▓░░░░░░  1.2 GB   ⌄ │
│    (✓ Safe)  What you lose: nothing                      │
│ ── expanded ──────────────────────────────────────────── │
│    Apps rebuild these when needed.          3,104 files  │
│    C:\Users\…\Temp\a1b2.tmp               12 KB   3 Oct  │
│    Show 100 more                                         │
╰──────────────────────────────────────────────────────────╯
```

## Per tab

- **Home:** shows a status sentence, the junk hero figure with "Clean safe
  items: X" (primary) and Review all, drive free-space bars, and any health
  checks that need attention. Checks that are OK collapse into "N checks OK".
  Each check's details, its "as of" time, and the read-only note are inside
  that check's expander. What grew shows only the biggest few changes, with
  See all.
- **Disk:** shows the drive picker with Scan (primary until a scan exists,
  then a secondary Scan again), one summary line (drive, size, files), the
  breadcrumbs, the folder list with size bars, and the grey treemap. Behind
  disclosure: size on disk, scan method (and the fast-scan admin notice),
  skipped items, and explanations for the hard-link and lock flags (the icons
  stay visible). "Send to cleaner" and "Add to exclusions" are in the row's
  actions.
- **Cleaner:** shows the found total, group panels with their totals, and
  collapsed rule rows (tier chip and What you lose always visible), plus the
  action bar with "Clean X" (primary). The admin notice stays visible because
  it changes what gets cleaned. The expanded row has the reason, the exact
  files, and the hidden and admin details. Advanced holds Deep clean and rule
  problems.
- **Privacy:** shows each Windows trace with a checkbox, its name, item count,
  and What you lose, plus "Clear selected" (primary). What each trace is goes
  in its expanded row. The confirm dialog keeps "these don't go to the
  recovery bin".
- **Files:** shows Look in, the sub-tabs (Large and old files / Duplicates),
  and one primary per sub-tab ("Find…", then "Review N picked"). Results show
  name, size bar, size, and age. Size, age, and kind filters are in an
  "Options" expander whose summary shows the current values. Full paths,
  modified dates, and duplicate group details are in expanded rows.
- **What grew:** shows the hero change ("+4.2 GB on C: since 3 Oct") and
  "Grew the most" with size bars. It has no primary action. The snapshot
  pickers move into a "Compare other scans" expander that defaults to the
  latest two. "Shrank the most" is collapsed, and folder paths and notes are
  in expanded rows. "Show in Disk" and "Clean: rule" stay as row actions.
- **Startup:** shows the apps with a switch, name, and publisher, and "N of M
  turned on". It has no primary action. The "All users" lock tag stays visible
  because it explains a disabled switch. The command line, source, and
  turned-off date are in the expanded row. Advanced holds the read-only
  services list.
- **Updates:** shows "N updates available" and each app with a checkbox, its
  name, and "1.2 → 1.3", plus "Update selected" (primary). The package ID and
  per-app result messages are in expanded rows. "winget isn't installed" is
  the empty state, worded as "Windows Package Manager".
- **Recover:** shows "In the bin: X" with one usage bar, and batches newest
  first (date, source, size) with Restore all. When files are picked inside a
  batch, the action bar shows "Restore N files" (primary). Per-drive usage and
  limits are inside the usage panel's expander. Original paths and tier chips
  are in the expanded batch. Delete batch and Empty bin are neutral and
  secondary.
- **Settings:** uses Windows settings rows with switches, saved at once:
  Appearance, Cleaning, Recovery bin, Scheduled cleaning (it keeps its own
  Save), and Tray monitor. It has no primary action. Advanced holds
  Exclusions, Custom cleaning rules, Secure delete, Scan snapshots, Health
  check thresholds, and Your data.

## Not done yet

The tokens in `app.css` are still the old "Ledger" values. Home has the old
layout restyle, but none of the screens use this palette yet. Still to do:

- Put the Graphite values into all three theme blocks of `app.css`, and add
  `--border-strong`, `--radius-pill`, and the motion tokens.
- Apply the scales, corners, motion, size bars, tier row layout, and
  disclosure patterns to the screens.
- Treemap (`Treemap.svelte`): replace the hardcoded stroke and fill colors
  with theme greys, and outline the selected block in `--focus`.
- `.btn.danger` and `.btn.danger.primary` use `--tier-red`, which breaks the
  tier rule, and the white text on it is below 4.5:1 in dark mode. Make
  destructive buttons neutral.
- Replace hardcoded colors and radii with tokens (for example the `Modal`
  backdrop and shadow, and the 6px button radius).
