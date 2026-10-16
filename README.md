# WinFastCleaner

A fast, safe, transparent disk cleaner and system health tool for Windows 10
and 11 (x64). WinFastCleaner is an open-source alternative to CCleaner. It is
not affiliated with or endorsed by CCleaner or Piriform/Gen Digital.

> **Status: early development (Phase 1 of 6).** The disk scanner and Disk view
> work today. **The cleaner is not built yet, so the app deletes nothing.**
> See [Roadmap](#roadmap).

- **Fast.** On NTFS drives, when run as administrator, it reads the Master
  File Table directly, so a full drive scan takes seconds. Otherwise it falls
  back to a parallel directory walk.
- **Visible.** You can always see what is in each folder and its real size,
  both logical size and size on disk. Hard links are counted once, and any
  folder whose size would otherwise be misleading is flagged.
- **Safe.** Every cleanable item is color-coded by risk. Nothing personal is
  ever selected automatically, and deleted items go to a recovery bin you can
  restore from.
- **Honest.** No ads, no upsells, no accounts, and **no telemetry**. It works
  fully offline.

## What it deletes, and what it never deletes

*The cleaner arrives in Phase 2. This section describes the rules it is being
built to follow. These rules are enforced in the core library, not only in
the UI.*

WinFastCleaner deletes only:

1. Files matched by a **cleaning rule**: plain data files, one per app or
   area (Windows temp, browser caches, crash dumps, and so on), that list
   exact locations and patterns. You can read every rule in [`rules/`](rules/).
2. Files **you pick yourself** in the disk view, the large/old file finder,
   or the duplicate finder. These are always treated as red tier (see below).

It never deletes anything inside these locations, even if a rule or a bug
asks it to:

- `C:\Windows`, except the specific temp, log, and update-cache subfolders a
  rule lists
- `C:\Program Files` and `C:\Program Files (x86)`
- `C:\ProgramData`, except the specific caches a rule lists
- `WinSxS` and `System32`
- the root of any drive, or the root of a user profile
- boot and recovery partitions, and the app's own install folder

It never follows symlinks, junctions, or mount points while deleting. Your
**exclusions list always wins** over every rule and selection. It never
touches the registry and never changes system settings.

## The three safety tiers

| Tier | Meaning | Selected by default | Where deleted files go |
|---|---|---|---|
| 🟢 **Green (safe)** | Regenerates automatically, no impact for you. Examples: temp files, browser caches, crash dumps, thumbnail cache. | Yes | Deleted directly (or to the recovery bin if you turn that on) |
| 🟡 **Yellow (optional)** | Safe, but has a cost, such as a slower next launch or a re-download. Examples: download caches, old Windows Update files, shader caches, `node_modules`, `.gradle`, `.m2`, log files. | No | Recovery bin |
| 🔴 **Red (review)** | Your own files. Examples: large files in Downloads, old installers, duplicates. | **Never** | Recovery bin |

Only paths matched by a rule file can be green or yellow. Anything you select
by hand is red. Red items are never included in scheduled cleaning. Every item
shows why it got its color and what you lose by deleting it, and you can
preview the exact file list before anything is removed.

## The recovery bin

Deleted yellow and red items aren't routed through the Windows Recycle Bin,
which copies files and is slow. They are **moved** into a hidden folder on
the same drive (`X:\.winfastcleaner-recovery\`), which is an instant rename.
Each cleanup is a batch you can restore whole or file by file from the
**Recover** tab. When the bin goes over its limits, the oldest batches are
permanently deleted first. The defaults are 5 GB or 10% of the drive
(whichever is smaller), 5 batches, and 14 days.

## Privacy

- **No telemetry, analytics, crash reporting, or update pings.** The app
  makes no network requests at all.
- The only external program it runs is Windows' own `winget`, and only when
  you use the Updates tab (Phase 5).
- Settings, scan snapshots, and logs stay on your machine in
  `%LOCALAPPDATA%\WinFastCleaner\`. Logs contain paths and sizes only, and
  are rotated.
- Privacy cleaning (Phase 4) only deletes browser files. It never reads,
  decrypts, exports, or logs their contents.

## Roadmap

| Phase | Scope | Status |
|---|---|---|
| 1 | Workspace, scanner (parallel walk and NTFS MFT), Disk tab with tree and treemap, scan snapshots | **In progress** |
| 2 | Rule files, built-in rules, tiered Cleaner tab, parallel deletion, recovery bin, exclusions, safety rules, `--dry-run` | Planned |
| 3 | Large/old file finder, duplicate finder, "What grew" | Planned |
| 4 | Privacy cleaning, secure delete | Planned |
| 5 | Startup manager, winget updater, health check, Home dashboard | Planned |
| 6 | Scheduled cleaning, tray monitor, settings, installer, docs | Planned |

## Using it today (Phase 1)

There are no installers yet. Build from source as described below. The CLI
can scan a folder or drive:

```powershell
wfc drives                     # list drives with free/used space
wfc scan C:\                   # scan, print the biggest folders
wfc scan D:\Projects --top 30 --depth 2
wfc scan C:\ --json > scan.json
```

Run from an elevated (administrator) terminal to use the fast MFT scan on
NTFS drives. Without admin rights it uses the parallel walker automatically.

### Windows SmartScreen

Builds are **not code-signed** yet, so Windows SmartScreen will warn that the
app is from an "unknown publisher" when you run a downloaded build. That is
expected for unsigned open-source software. Release builds come from CI and
publish SHA-256 checksums that you can verify with
`Get-FileHash .\file.exe -Algorithm SHA256`.

## Building from source

Prerequisites (Windows): [Rust](https://rustup.rs) (the exact version is
pinned in `rust-toolchain.toml`), [Node.js 22](https://nodejs.org), the
Microsoft C++ Build Tools, and WebView2 (preinstalled on Windows 10/11).

```powershell
# CLI
cargo run -p cleaner-cli --release -- scan C:\

# Desktop app (development)
cd app
npm ci
npm run tauri dev
```

Run the tests with `cargo test --workspace` and `npm test --prefix app`. See
[CONTRIBUTING.md](CONTRIBUTING.md) for details, including the NTFS image tests
that run on Linux.

## Project layout

| Path | What it is |
|---|---|
| `crates/core` | Rust library: scanner, MFT reader, snapshots, and later the rules engine, deletion, and recovery bin. No UI code. |
| `crates/cli` | `wfc` command-line tool built on `core`. Scheduled cleaning will use it. |
| `app/` | Tauri desktop app (Svelte + TypeScript frontend in `app/src`, Rust backend in `app/src-tauri`) |
| `rules/` | Built-in cleaning rule files (Phase 2) |
| `docs/` | Architecture, safety model, and (Phase 2) rule file format |

## Security

Please report vulnerabilities privately. See [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE). All code and built-in rules are written from scratch. No code
or rule files are copied from other cleaners.
