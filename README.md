# WinFastCleaner

A fast, safe, transparent disk cleaner and system health tool for Windows 10
and 11 (x64). WinFastCleaner is an open-source alternative to CCleaner. It is
not affiliated with or endorsed by CCleaner or Piriform/Gen Digital.

> **Status: early development (Phase 2 of 6).** The disk scanner, Disk view,
> cleaner, and recovery bin work today. It has not been tested widely on real
> PCs yet, so preview what it will clean and keep the recovery bin on. See
> [Roadmap](#roadmap).

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

These rules are enforced in the core library, not only in the UI, so they
apply equally to the desktop app, the CLI, and scheduled cleaning. The full
details are in the [safety model](docs/safety-model.md).

WinFastCleaner deletes only:

1. Files matched by a **cleaning rule**: plain data files, one per app or
   area (Windows temp, browser caches, crash dumps, and so on), that list
   exact locations and patterns. You can read every rule in [`rules/`](rules/).
2. Files **you pick yourself** in the disk view, the large/old file finder,
   or the duplicate finder. These are always treated as red tier (see below).

It never deletes anything inside these locations, even if a rule or a bug
asks it to:

- `C:\Windows`, except its `Temp`, `Logs`, crash dump, and Windows Update
  download folders
- `C:\Program Files` and `C:\Program Files (x86)`
- `C:\ProgramData`, except Windows error reports
- `WinSxS`, `System32`, `SysWOW64`, and other Windows system folders
  wherever they are
- the root of any drive, or the root of a user profile
- boot and recovery folders, `pagefile.sys`, `hiberfil.sys`, and other boot
  files
- your saved credentials, encryption keys, certificates, registry files, and
  Microsoft Store app data
- the app's own files

These protected files are left out of the cleaner's results entirely, so they
can't be selected by accident. (The Disk view still shows them, with a lock
icon, so sizes add up.)

It never follows symlinks, junctions, or mount points while deleting. Each
file is checked again right before it is removed, and anything that changed
since the preview, is in use, or is denied is skipped and reported. Your
**exclusions list always wins** over every rule and selection. It never
touches the registry and never changes system settings.

### Built-in rules

These are all the locations the cleaner looks in out of the box (each rule
file is in [`rules/`](rules/); `*` matches any folder name, such as each
browser profile):

| Item | Tier | Locations | Notes |
|---|---|---|---|
| Temporary files (your account) | Safe | `%TEMP%` | older than 1 day |
| Temporary files (Windows) | Safe | `%SYSTEMROOT%\Temp` | older than 1 day; needs admin |
| Thumbnail cache | Safe | `%LOCALAPPDATA%\Microsoft\Windows\Explorer` | only `thumbcache_*.db` |
| Error reports (your account) | Safe | `%LOCALAPPDATA%\Microsoft\Windows\WER\ReportArchive`<br>`%LOCALAPPDATA%\Microsoft\Windows\WER\ReportQueue` |  |
| Error reports (Windows) | Safe | `%PROGRAMDATA%\Microsoft\Windows\WER\ReportArchive`<br>`%PROGRAMDATA%\Microsoft\Windows\WER\ReportQueue`<br>`%PROGRAMDATA%\Microsoft\Windows\WER\Temp` | needs admin |
| Program crash dumps | Safe | `%LOCALAPPDATA%\CrashDumps` |  |
| Windows crash dumps | Safe | `%SYSTEMROOT%\Minidump`<br>`%SYSTEMROOT%\LiveKernelReports`<br>`%SYSTEMROOT%\MEMORY.DMP` | needs admin |
| Internet cache (Windows) | Safe | `%LOCALAPPDATA%\Microsoft\Windows\INetCache` |  |
| DirectX shader cache | Optional | `%LOCALAPPDATA%\D3DSCache` |  |
| Delivery Optimization cache | Optional | `%SYSTEMROOT%\ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache` | needs admin |
| Windows Update downloads | Optional | `%SYSTEMROOT%\SoftwareDistribution\Download` | older than 10 days; needs admin |
| Windows log files | Optional | `%SYSTEMROOT%\Logs` | only `**/*.log`, `**/*.etl`, `**/*.cab`; older than 30 days; needs admin |
| Google Chrome cache | Safe | `%LOCALAPPDATA%\Google\Chrome\User Data\*\Cache`<br>`%LOCALAPPDATA%\Google\Chrome\User Data\*\Code Cache`<br>`%LOCALAPPDATA%\Google\Chrome\User Data\*\GPUCache`<br>`%LOCALAPPDATA%\Google\Chrome\User Data\ShaderCache`<br>`%LOCALAPPDATA%\Google\Chrome\User Data\GrShaderCache` | skipped while chrome.exe runs |
| Microsoft Edge cache | Safe | `%LOCALAPPDATA%\Microsoft\Edge\User Data\*\Cache`<br>`%LOCALAPPDATA%\Microsoft\Edge\User Data\*\Code Cache`<br>`%LOCALAPPDATA%\Microsoft\Edge\User Data\*\GPUCache`<br>`%LOCALAPPDATA%\Microsoft\Edge\User Data\ShaderCache`<br>`%LOCALAPPDATA%\Microsoft\Edge\User Data\GrShaderCache` | skipped while msedge.exe runs |
| Brave cache | Safe | `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\*\Cache`<br>`%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\*\Code Cache`<br>`%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\*\GPUCache`<br>`%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\ShaderCache` | skipped while brave.exe runs |
| Opera cache | Safe | `%LOCALAPPDATA%\Opera Software\Opera Stable\Cache`<br>`%LOCALAPPDATA%\Opera Software\Opera Stable\*\Cache`<br>`%APPDATA%\Opera Software\Opera Stable\Code Cache`<br>`%APPDATA%\Opera Software\Opera Stable\GPUCache` | skipped while opera.exe runs |
| Firefox cache | Safe | `%LOCALAPPDATA%\Mozilla\Firefox\Profiles\*\cache2`<br>`%LOCALAPPDATA%\Mozilla\Firefox\Profiles\*\startupCache` | skipped while firefox.exe runs |
| Discord cache | Safe | `%APPDATA%\discord\Cache`<br>`%APPDATA%\discord\Code Cache`<br>`%APPDATA%\discord\GPUCache` | skipped while discord.exe runs |
| Microsoft Teams (classic) cache | Safe | `%APPDATA%\Microsoft\Teams\Cache`<br>`%APPDATA%\Microsoft\Teams\Code Cache`<br>`%APPDATA%\Microsoft\Teams\GPUCache`<br>`%APPDATA%\Microsoft\Teams\Service Worker\CacheStorage` | skipped while teams.exe runs |
| Visual Studio Code cache | Safe | `%APPDATA%\Code\Cache`<br>`%APPDATA%\Code\CachedData`<br>`%APPDATA%\Code\Code Cache`<br>`%APPDATA%\Code\GPUCache`<br>`%APPDATA%\Code\CachedExtensionVSIXs` | skipped while code.exe runs |
| Steam web cache | Safe | `%LOCALAPPDATA%\Steam\htmlcache` | skipped while steam.exe or steamwebhelper.exe runs |
| Spotify cache | Optional | `%LOCALAPPDATA%\Spotify\Storage` | skipped while spotify.exe runs |
| npm cache | Optional | `%LOCALAPPDATA%\npm-cache`<br>`%APPDATA%\npm-cache` |  |
| Yarn cache | Optional | `%LOCALAPPDATA%\Yarn\Cache` |  |
| pip cache | Optional | `%LOCALAPPDATA%\pip\Cache` |  |
| NuGet HTTP cache | Optional | `%LOCALAPPDATA%\NuGet\v3-cache`<br>`%LOCALAPPDATA%\NuGet\http-cache` |  |
| Gradle caches | Optional | `%USERPROFILE%\.gradle\caches` | skipped while java.exe or javaw.exe runs |
| Maven local repository | Optional | `%USERPROFILE%\.m2\repository` | skipped while java.exe or javaw.exe runs |
| Cargo download cache | Optional | `%USERPROFILE%\.cargo\registry\cache` |  |
| Go build cache | Optional | `%LOCALAPPDATA%\go-build` |  |
| ~/.cache folder | Optional | `%USERPROFILE%\.cache` |  |

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
which copies files and is slow. They are **moved** into a folder on the same
drive, which is an instant rename: inside the app's private data folder for
your system drive, and a hidden `X:\.winfastcleaner-recovery\` folder (with
permissions only for you) on other drives. Each cleanup is a batch you can
restore whole or file by file from the **Recover** tab. If a file with the
same name is back at the original location, you choose: skip, keep both, or
overwrite (the existing file goes to the bin first). When the bin goes over
its limits, the oldest batches are permanently deleted first. The defaults
are 5 GB or 10% of the drive (whichever is smaller), 5 batches, and 14 days,
and you can change them in **Settings**.

Moving files to the bin doesn't free their space until the bin is emptied or
the batch expires. Safe (green) items skip the bin by default so their space
is freed right away; turn on **Send safe items to the recovery bin too** in
Settings if you prefer.

## Privacy

- **No telemetry, analytics, crash reporting, or update pings.** The app
  makes no network requests at all.
- The only external programs it runs are Windows' own `winget`, and only
  when you use the Updates tab (Phase 5), and `Dism.exe`, only when you click
  **Deep clean**.
- Settings, scan snapshots, and logs stay on your machine in
  `%LOCALAPPDATA%\WinFastCleaner\`. Logs contain paths and sizes only, and
  are rotated.
- Privacy cleaning (Phase 4) only deletes browser files. It never reads,
  decrypts, exports, or logs their contents.

## Roadmap

| Phase | Scope | Status |
|---|---|---|
| 1 | Workspace, scanner (parallel walk and NTFS MFT), Disk tab with tree and treemap, scan snapshots | **Built** |
| 2 | Rule files, built-in rules, tiered Cleaner tab, parallel deletion, recovery bin, exclusions, safety rules, `--dry-run` | **Built** |
| 3 | Large/old file finder, duplicate finder, "What grew" | Planned |
| 4 | Privacy cleaning, secure delete | Planned |
| 5 | Startup manager, winget updater, health check, Home dashboard | Planned |
| 6 | Scheduled cleaning, tray monitor, settings, installer, docs | Planned |

## Using it today

There are no installers yet (they arrive in Phase 6). Build from source as
described below.

### Desktop app

- **Disk** opens first and immediately scans your system drive. Pick another
  drive or **Scan a folder…**. Click folders in the tree, treemap, or table
  to drill in. Each item shows its size, size on disk, share of its folder
  and of the drive, file count, and last-modified date. Right-click (or use
  **⋯**) to show an item in File Explorer, copy its path, **send it to the
  cleaner** (it goes to the recovery bin, as a red-tier item), or **add it to
  your exclusions**. Protected items show a lock and can't be sent to the
  cleaner.
- **Cleaner** finds junk with the [rules](rules/), grouped by app or area.
  Every item shows its tier (Safe, Optional), size, file count, why it is
  safe, and what you lose. Expand any item to see (and untick) the exact
  files. **Clean 4.2 GB (1,382 files)** shows the exact list's totals before
  anything happens; afterwards you see the space freed and any files that
  were skipped, with reasons. Items that need administrator rights or a
  closed app are shown but skipped, and say why. **Deep clean** runs
  Windows' own component store cleanup (DISM) separately; it is slow and
  needs administrator rights.
- **Recover** lists recovery bin batches, newest first, with bin usage per
  drive. Restore a whole batch or chosen files, delete a batch, or empty the
  bin.
- **Settings**: send safe items to the bin too, recovery bin limits, your
  exclusions (folders, files, and patterns such as `*.psd`), and your custom
  rules folder.

When the app isn't running as administrator, it says so and offers **Restart
as administrator** for the fast MFT scan and admin-only cleaning.

### CLI (`wfc`)

```powershell
wfc drives                                  # drives with size and free space
wfc scan C:\                                # biggest folders and files
wfc scan D:\Projects --top 30 --depth 2     # more entries, two levels
wfc scan C:\ --json > scan.json             # machine-readable output

wfc rules list                              # every cleaning rule, with its tier
wfc clean --dry-run --list                  # exactly what would be cleaned (safe items)
wfc clean --tier yellow --dry-run           # safe and optional items
wfc clean --rule chrome.cache --rule windows.user-temp
wfc clean                                   # asks before cleaning; --yes to skip the question

wfc recover list                            # recovery bin batches, newest first
wfc recover show <BATCH>                    # the files in a batch
wfc recover restore <BATCH> [--item N] [--on-conflict skip|keep-both|overwrite]
wfc recover delete <BATCH>                  # delete one batch for good
wfc recover empty                           # empty the bin
```

`wfc clean` never cleans review (red) items, always shows what it found
before acting, and refuses to run without `--yes` when there is no terminal
to ask on. Run from an elevated (administrator) terminal to use the fast MFT
scan and to clean the admin-only items. Each scan saves a small snapshot of
folder sizes (used by "What grew" in Phase 3); pass `--no-snapshot` to skip
it.

### Your own cleaning rules

Put rule files in `%LOCALAPPDATA%\WinFastCleaner\rules\`. The format is
documented in [docs/rule-format.md](docs/rule-format.md); check a file with
`wfc rules check my-rules.toml`.

### What "size" means

- **Size** is the logical size. **On disk** is the space actually allocated.
  Small files stored inside the MFT take 0 bytes on disk, compressed and
  sparse files take less than their size, and cloud-only OneDrive files take
  none.
- **Hard links are counted once.** Folders that contain hard-linked files
  (for example `C:\Windows\WinSxS`) are marked with an icon, because File
  Explorer counts every name and so shows them as bigger than they are.
- **Links are never followed.** Junctions and symlinks show as links with
  0 bytes, so nothing is counted twice and scans can't loop.
- Folders that couldn't be read are marked, and the summary lists them.

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

# UI only, in a browser, with fake data (no Rust needed)
npm run dev:mock
```

Run the tests with `cargo test --workspace` and `npm test --prefix app`. See
[CONTRIBUTING.md](CONTRIBUTING.md) for details, including the NTFS image tests
that run on Linux.

## Project layout

| Path | What it is |
|---|---|
| `crates/core` | Rust library: scanner, MFT reader, snapshots, safety guard, rules engine, deletion, and recovery bin. No UI code. |
| `crates/cli` | `wfc` command-line tool built on `core`. Scheduled cleaning will use it. |
| `app/` | Tauri desktop app (Svelte + TypeScript frontend in `app/src`, Rust backend in `app/src-tauri`) |
| `rules/` | Built-in cleaning rule files ([format](docs/rule-format.md)) |
| `docs/` | [Architecture](docs/architecture.md), [safety model](docs/safety-model.md), and [rule file format](docs/rule-format.md) |
| `scripts/` | Developer scripts (NTFS test image) |

## Security

Please report vulnerabilities privately. See [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE). All code and built-in rules are written from scratch. No code
or rule files are copied from other cleaners.
