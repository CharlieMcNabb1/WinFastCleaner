# Architecture

```
┌───────────────────────────── app/ (Tauri) ─────────────────────────────┐
│  Svelte + TypeScript UI (app/src)                                      │
│        │  invoke() / Channel (only commands granted in capabilities/)  │
│        ▼                                                               │
│  Rust commands (app/src-tauri/src: commands, cleaning, recover, …)     │
└────────┬───────────────────────────────────────────────────────────────┘
         │                         ┌──────────── crates/cli ─────────────┐
         │                         │  `wfc` command-line tool, and       │
         │                         │  `wfcw` (scheduled clean, no window)│
         │                         └──────────────┬──────────────────────┘
         │                         ┌──────────── crates/tray ────────────┐
         │                         │  `wfc-tray` tray monitor (Win32)    │
         ▼                         └──────────────┬──────────────────────┘
┌──────────────────────────── crates/core ───────────────────────────────┐
│  scan.rs      picks the MFT reader or the walker, progress, summary    │
│  mft/         NTFS Master File Table reader (read-only)                │
│  walk.rs      parallel directory walker                                │
│  tree.rs      compact in-memory folder tree                            │
│  snapshot.rs  folder-size snapshots for "What grew"                    │
│  platform.rs  drives, free space, elevation, Win32 helpers             │
│  safety.rs    the guard: protected paths, checked on final paths       │
│  rules.rs     rule files: load, validate, expand paths                 │
│  glob.rs      case-insensitive * ? ** patterns                         │
│  cleaner.rs   junk scan → exact plan → link-safe execute               │
│  fsops.rs     open without following links; delete/rename by handle   │
│  recovery.rs  recovery bin: write-ahead index, restore, eviction       │
│  settings.rs  settings, exclusions, bin limits                         │
│  finder.rs    large and old files (from a scan tree)                   │
│  dupes.rs     duplicates: size → partial BLAKE3 → full BLAKE3          │
│  grew.rs      "What grew": compares two snapshots                      │
│  privacy.rs   Windows traces: recent docs, jump lists, Run, clipboard  │
│  startup.rs   startup items: list, turn off/on (never delete)          │
│  services.rs  auto-start services (read-only)                          │
│  winget.rs    app updates: find winget, parse its table, upgrade       │
│  health.rs    health checks: pure evaluate() + cached inputs           │
│  schedule.rs  scheduled cleaning: settings, task XML, the run          │
│  monitor.rs   tray notifications: what is due, once a day per kind     │
│  integration.rs  the task and the tray's sign-in entry; removal        │
│  wincom.rs    COM: Task Scheduler, WMI, shell links (Windows)          │
│  winreg.rs    registry access (Windows)                                │
│  log.rs       local cleanup log (JSON lines, rotated)                  │
└────────────────────────────────────────────────────────────────────────┘
```

Every safety-relevant decision lives in `crates/core`, so the CLI,
scheduled cleaning, and the desktop app all behave the same way. The UI and
CLI are thin layers.

## Scanning

`scan::scan(path, options, progress, cancel)` returns a `Tree` and a
`ScanSummary`. It chooses a method:

1. **MFT reader** (Windows, NTFS, local drive letter, running as admin).
   It opens `\\.\C:` read-only and parses the Master File Table directly.
2. **Parallel walker** (everything else, or if the MFT can't be used). The
   summary records why the MFT reader wasn't used, and the UI shows it.

Both produce the same `Tree`, so everything downstream is method-agnostic.

### MFT reader (`crates/core/src/mft`)

1. Parse the boot sector: cluster size, MFT location, record size.
2. Read record 0 (`$MFT` itself) to get the MFT's data runs, including
   extents listed in its `$ATTRIBUTE_LIST` when the MFT is fragmented.
3. Read the MFT in 4 MB sequential chunks on a reader thread. Parse each
   chunk's records in parallel (rayon): apply update-sequence fixups (torn
   records are skipped), then read `$STANDARD_INFORMATION` (modified time,
   attributes), `$FILE_NAME` (parent reference and name; DOS 8.3 duplicates
   skipped), `$DATA` (logical size of the main stream; allocated size of all
   streams, using the real allocation for sparse and compressed streams), and
   `$REPARSE_POINT` (links).
4. Extension records are merged into their base record, so files with many
   hard links or a fragmented attribute list are complete.
5. Link names to parents using the parent's record number **and sequence
   number**. A stale reference to a deleted, reused record becomes an orphan
   and is dropped instead of being attached to the wrong folder.

Every read is bounds-checked. A corrupt record is skipped, never trusted. The
reader takes any `Read + Seek`, which is how it is tested against real NTFS
images on Linux.

Measured on a synthetic NTFS image with 202,000 records: about 115 ms in a
release build with a warm cache. A 1 TB system drive typically has 1–3
million records, so parsing takes about 1–2 seconds plus the time to read the
MFT from disk. To measure on a real drive, run this from an elevated
terminal:

```powershell
cargo run --release -p cleaner-core --example mft_bench -- \\.\C:
```

### Parallel walker (`crates/core/src/walk.rs`)

- Uses rayon work-stealing with one task per directory, so large and deep
  trees spread across cores.
- **Never follows links.** Symlinks, junctions, and mount points
  ("name-surrogate" reparse points) are listed with the `LINK` flag but not
  entered. Other reparse folders, such as OneDrive's, are real folders and
  are entered.
- **Never opens file contents.** Cloud placeholders are detected from their
  attributes and reported with 0 bytes on disk, so they are never downloaded.
- Size on disk: Windows directory listings only give the logical size, so
  the walker rounds up to the cluster size. For compressed, sparse, and other
  special files it asks Windows for the exact value. (The MFT reader is
  exact for every file.)
- Hard links: on Windows, checking a file's link count costs one extra system
  call. The walker does it under the Windows folder, where almost all hard
  links live (WinSxS). Elsewhere it assumes one name per file. Run as admin
  for exact hard-link handling everywhere.
- Unreadable folders are flagged and listed in the summary. They never abort
  the scan.

### The tree (`crates/core/src/tree.rs`)

Scanners push raw entries into a `TreeBuilder`. `finish()` then:

1. builds children lists from parent pointers,
2. keeps only entries reachable from the root (dropping orphans and any
   cycles in corrupt data),
3. counts each hard-linked file once, at its first name in name order, and
   flags the other names (`HARDLINK_DUP`) and every folder above them
   (`HAS_HARDLINKS`, shown in the UI with an explanation, because File
   Explorer counts each name and so shows larger sizes),
4. adds up sizes, file and folder counts, and newest modified time,
5. sorts children largest first.

Nodes live in flat arrays (about 56 bytes each plus the name), which keeps a
multi-million-entry scan to a few hundred MB.

## Cleaning

The cleaner is split so that what the user previews is exactly what is
acted on (see the [safety model](safety-model.md) for every check):

1. `rules::load` reads the built-in rule files (compiled in) and the user's
   rules folder, validating each rule ([format](rule-format.md)).
2. `cleaner::scan(rules, context, cancel)` finds files for each rule in
   parallel. The `Context` holds the guard, the variables (`%TEMP%`, …), the
   exclusions, whether the process is elevated, and the running programs.
   Each rule gets a status: ready, needs admin, app running, or nothing
   found.
3. `cleaner::plan_rules` (selected rules minus unticked files) or
   `cleaner::plan_manual` (Disk view picks, always red) builds a `CleanPlan`:
   the exact files with sizes and modified times.
4. `cleaner::execute(plan, …)` records the recovery batch first, then acts on
   each file in parallel through `fsops` (open without following links,
   re-check, delete or rename through the same handle), removes emptied
   folders, applies the bin's limits, writes the log, and measures the free
   space gained.

In the app, the backend keeps the junk scan and the previewed plan; the UI
only sends rule ids, file indexes, and the plan id. A plan can be executed
once. `wfc clean` runs the same steps, prints the scan, and asks before
executing (`--dry-run` stops after the plan).

## Recovery bin

`recovery::RecoveryStore` keeps an index (`recovery\index.json` in the data
folder) of batches and their files. On the data folder's drive the bin is
`recovery\bin` in the data folder; on other drives it is
`X:\.winfastcleaner-recovery\<user>`, a folder with private permissions
(`winsec.rs`). Index changes are serialized within the process and locked
against other processes, and a batch is written before any file moves, so a
crash can't leave the index wrong. Restore, delete, empty, and eviction all
act through the same link-safe file operations.

## Privacy and secure delete

`privacy::survey` counts each trace and `privacy::clear` removes the chosen
ones (see the [safety model](safety-model.md#privacy-cleaning)). Locations
are a parameter (`privacy::Locations`), so tests point them at temp folders
and a throwaway registry key and never touch the real lists.

Secure delete is `fsops::open_for_wipe` + `Opened::wipe` (write zeros over
the file's length through the handle, flush) + delete through the same
handle. `cleaner::execute` uses it for green and yellow items when
`ExecOptions::secure` is set; red items still go to the bin.
`RecoveryStore::delete_batch` and `empty` take a `secure` flag.

## File finders and "What grew"

Both finders work on a finished scan's `Tree`, so after a scan (or reusing
the Disk tab's scan of the same root) they start instantly.

- `finder::find` walks the tree's file nodes, keeps those over the size
  limit (and, for old files, not modified since the cutoff), largest first,
  then checks each candidate against the guard and the exclusions and reads
  fresh metadata, including the last-access time when Windows records it.
- `dupes::find` groups files by size, then hashes the first and last 16 KB
  of each remaining candidate (files up to 32 KB are hashed whole here), then
  hashes whole files with BLAKE3, in parallel. Only the full hash decides.
  Files are opened with `fsops::open_read`, which never follows links and
  refuses cloud placeholders (reading those would download them). Several
  names of one hard-linked file are recognized by their file ID and are not
  reported as copies.
- `dupes::plan_removal` turns picked copies into a red-tier plan, and refuses
  if any group would lose every copy or its kept copy is gone.
- `grew::compare` matches two snapshots' folders by path. A folder missing on
  one side was under the snapshot threshold (50 MB). A change is listed at
  the most specific folder that explains it: a folder is hidden when one of
  its subfolders accounts for at least 75% of its change.

## Startup manager, updates, and health check

- `startup::list` reads the Run keys (`HKCU`, `HKLM`, and `HKLM`'s 32-bit
  view), the two Startup folders (resolving `.lnk` targets through
  `IShellLink`), and, through Task Scheduler's COM API, tasks outside
  `\Microsoft\` that have a sign-in trigger. Each item's on/off state comes
  from Task Manager's `Explorer\StartupApproved` values (or the task's own
  flag), and its publisher from the program's version information, read as
  data. `startup::set_enabled` writes only that switch (12 bytes: `02` on,
  `03` and the time off) or the task's flag. Locations are a parameter, so
  tests use a throwaway `HKCU` key, a temporary folder, and a throwaway task.
- `services::auto_start` lists Win32 services set to start automatically
  whose program is outside the Windows folder. It only queries.
- `winget::list` runs `winget upgrade` and parses its table by column
  position (the header line above the dashes; display width counts East
  Asian wide characters as two), so translated headers work. `winget::upgrade`
  runs `winget upgrade --id <id> --exact --silent` with the agreements
  accepted and no interactivity, one app at a time.
- `health::evaluate` is a pure function from `health::Inputs` (drives, the
  cached junk and update summaries, the startup count, drive health, recovery
  bin usage) and the thresholds in settings to a list of checks.
  `health::read_disks` queries `MSFT_PhysicalDisk` (health status),
  `MSFT_StorageReliabilityCounter` (wear), and, where readable,
  `MSStorageDriver_FailurePredictData` (raw SMART: reallocated, pending, and
  uncorrectable sectors) through WMI.
- The app's junk scan saves `junk-summary.json` and the update check saves
  `updates-summary.json` in the data folder, so the health check (and later
  the tray) can use them without scanning again.

## Scheduled cleaning, the tray, and the installer

- `schedule::task_xml` builds the Task Scheduler definition: a calendar
  trigger whose start boundary is a fixed past date at the chosen local
  time (no time zone, so it follows the PC's clock), interactive token,
  least privilege, `StartWhenAvailable`, one hour limit, no overlapping
  runs. `wincom::register_task` registers it under `\WinFastCleaner`.
- `schedule::run` loads the settings and rules, selects rules with
  `select_rules` (never red), scans, plans, filters the plan for red again,
  executes, and writes `scheduled-last.json`. `wfc clean --scheduled` and
  `wfcw` (a GUI-subsystem binary, so no console window flashes) both call it.
- `wfc-tray` (`crates/tray`) is a plain Win32 program: a hidden top-level
  window (so Explorer's "taskbar created" broadcast re-adds the icon), a
  `Shell_NotifyIcon` icon with a menu, and a timer. Checks run on a worker
  thread (`health::gather` + `evaluate`, `monitor::alerts`) and post their
  result back to the window, which shows notifications through the icon.
- The installer is Tauri's NSIS bundle (`app/src-tauri/tauri.bundle.conf.json`),
  per machine, with `wfc.exe`, `wfcw.exe`, and `wfc-tray.exe` as extra
  binaries next to the app. `windows/hooks.nsh` closes the tray before an
  upgrade and runs `wfc remove-integration` before uninstalling.
  `scripts/build-installer.ps1` builds it and `scripts/test-installer.ps1`
  installs, checks, and uninstalls it in CI; `release.yml` publishes it on
  version tags with SHA-256 checksums as a draft release.

## Elevation

The app runs unelevated by default. Admin rights are needed for the fast
MFT scan, for rules marked `requires_admin`, and for Deep clean. The Disk
and Cleaner tabs say so and offer **Restart as administrator**, which
relaunches the app through UAC (one prompt for the session, as the spec
allows). Without admin, the walker is used and admin-only rules are skipped
with the reason, so nothing breaks.

## Snapshots

After each scan, `snapshot::save` writes the folders of at least 50 MB (up
to 12 levels deep) to
`%LOCALAPPDATA%\WinFastCleaner\snapshots\<root-key>-<UTC time>.snapshot.json`.
Files are written to a temporary name and then renamed, so a crash never
leaves a partial snapshot. Snapshots older than 90 days are pruned. Pruning
only deletes regular files with the snapshot suffix directly in that folder,
never links. Phase 3's "What grew" view compares two snapshots.

## Desktop app security

- **Commands:** `app/src-tauri/build.rs` declares the app's commands in a
  manifest, and `capabilities/main.json` grants exactly those, plus the
  native file and folder picker (`dialog:allow-open`). There are no shell, fs, http,
  or opener permissions. (The dialog plugin depends on the fs plugin, but
  none of its permissions are granted.)
- **CSP:** `default-src 'self'`, with no inline scripts or styles, no remote
  origins, and `object-src`, `frame-src`, `worker-src`, `base-uri`, and
  `form-action` set to none. Prototypes are frozen.
- **No raw paths for actions:** the UI refers to Disk view items by
  `(scanId, nodeId)` and to cleaner files by their index in the backend's
  junk scan. The backend resolves paths from its own data. The only paths
  the UI sends are the folder to scan, which is validated (absolute, no
  device paths, no NUL, length-limited), and exclusions, which can only
  protect files and are validated too.
- **No network:** `deny.toml` bans HTTP and TLS client crates. The only
  outside programs are `winget` (Updates tab) and `Dism.exe` (Deep clean),
  each started with fixed arguments and no shell. winget is taken from its
  fixed per-user location, never looked up through `PATH`, and only package
  ids from winget's own latest list, checked against a strict pattern, are
  passed back to it.
- WebView2 keeps its own browser data folder
  (`%LOCALAPPDATA%\io.github.winfastcleaner\`). The app loads no remote
  content, so this only holds WebView2's own caches.

## Testing

| What | Where | Runs on |
|---|---|---|
| Unit tests (tree, walker, MFT parsing, snapshots, formatting) | `crates/*/src` | Windows + Linux CI |
| Safety guard, rules, globs, settings, link-safe file operations | `crates/core/src/{safety,rules,glob,settings,fsops}.rs` | Windows + Linux CI |
| Cleaning and recovery in temp folders: protected paths, links and junctions, exclusions, locked files, running apps, crash recovery of the index, restore conflicts, eviction | `crates/core/src/{cleaner,recovery}.rs` | Windows + Linux CI |
| MFT reader vs. walker on a real NTFS image (hard links, attribute lists, sparse files, ADS, junctions, symlinks, Unicode) | `crates/core/tests/ntfs_image.rs` + `scripts/ntfs-test-image.sh` | Linux CI |
| MFT scan vs. walker on a real Windows folder, plus a full system-drive MFT scan | `crates/core/tests/windows_volume.rs` | Windows CI (elevated) |
| File finders and comparisons: largest/oldest files, duplicates decided by full contents (same start and end but different middle are not duplicates), hard links, never removing every copy, growth at the right folder | `crates/core/src/{finder,dupes,grew}.rs` | Windows + Linux CI |
| Startup items turned off and on in a throwaway `HKCU` key and temp folder (Run values and shortcuts are never deleted); reading the real lists, services, and drive health; a throwaway scheduled task registered, toggled, and deleted | `crates/core/src/{startup,wincom}.rs`, `health.rs` | Windows CI |
| winget table parsing (translated and wide-character headers, truncated ids, second tables), id validation, health thresholds, SMART parsing | `crates/core/src/{winget,health}.rs` | Windows + Linux CI |
| CLI end-to-end, including dry run, clean, restore, finders, and What grew in temp folders | `crates/cli/tests/cli.rs` | Windows + Linux CI |
| Scheduled cleaning: rule selection never includes red, task XML, safe-only and optional runs through `wfc` and `wfcw`, and a real Task Scheduler task that runs and cleans safe items only | `crates/core/src/schedule.rs`, `crates/cli/tests/cli.rs` | Windows + Linux CI (the real task on Windows) |
| Tray notifications: thresholds, once a day per kind, only newly red checks | `crates/core/src/monitor.rs` | Windows + Linux CI |
| The installer installs, works, and uninstalls cleanly (task and sign-in entry removed, data folder kept) | `scripts/test-installer.ps1` | Windows CI |
| Frontend formatting, treemap layout, cleaner selection logic, health/startup/update helpers, next scheduled run | `app/src/lib/*.test.ts` | Windows CI |
| UI in a browser with fake data | `npm run dev:mock` | Local |
