# WinFastCleaner

A fast, safe, transparent disk cleaner and system health tool for Windows 10
and 11 (x64). WinFastCleaner is an open-source alternative to CCleaner. It is
not affiliated with or endorsed by CCleaner or Piriform/Gen Digital.

> **Status: all six build phases are done, but it is new.** It has not been
> tested widely on real PCs yet, so preview what it will clean and keep the
> recovery bin on. See [Roadmap](#roadmap).

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
"cleans" or "optimizes" the registry and never changes system settings. The
only registry data it ever removes is your own Run dialog history, and only
when you clear it in the Privacy tab.

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

On Linux the cleaner uses [`rules/linux.toml`](rules/linux.toml) instead.
Another account's files (for example in `/tmp`) are never listed or cleaned.

| Item | Tier | Locations | Notes |
|---|---|---|---|
| Temporary files | Safe | `%TEMP%` (`/tmp`) | older than 7 days |
| Thumbnail cache | Safe | `~/.cache/thumbnails` |  |
| Google Chrome cache | Safe | `~/.cache/google-chrome/*/Cache`<br>`~/.cache/google-chrome/*/Code Cache`<br>`~/.config/google-chrome/*/GPUCache`<br>`~/.config/google-chrome/ShaderCache`<br>`~/.config/google-chrome/GrShaderCache` | skipped while chrome runs |
| Chromium cache | Safe | `~/.cache/chromium/*/Cache`<br>`~/.cache/chromium/*/Code Cache`<br>`~/.config/chromium/*/GPUCache`<br>`~/.config/chromium/ShaderCache`<br>`~/.config/chromium/GrShaderCache` | skipped while chromium runs |
| Microsoft Edge cache | Safe | `~/.cache/microsoft-edge/*/Cache`<br>`~/.cache/microsoft-edge/*/Code Cache`<br>`~/.config/microsoft-edge/*/GPUCache`<br>`~/.config/microsoft-edge/ShaderCache`<br>`~/.config/microsoft-edge/GrShaderCache` | skipped while msedge runs |
| Brave cache | Safe | `~/.cache/BraveSoftware/Brave-Browser/*/Cache`<br>`~/.cache/BraveSoftware/Brave-Browser/*/Code Cache`<br>`~/.config/BraveSoftware/Brave-Browser/*/GPUCache`<br>`~/.config/BraveSoftware/Brave-Browser/ShaderCache` | skipped while brave runs |
| Firefox cache | Safe | `~/.cache/mozilla/firefox/*/cache2`<br>`~/.cache/mozilla/firefox/*/startupCache` | skipped while firefox runs |
| Discord cache | Safe | `~/.config/discord/Cache`<br>`~/.config/discord/Code Cache`<br>`~/.config/discord/GPUCache` | skipped while discord runs |
| Visual Studio Code cache | Safe | `~/.config/Code/Cache`<br>`~/.config/Code/CachedData`<br>`~/.config/Code/Code Cache`<br>`~/.config/Code/GPUCache`<br>`~/.config/Code/CachedExtensionVSIXs` | skipped while code runs |
| Steam web cache | Safe | `~/.local/share/Steam/config/htmlcache` | skipped while steam or steamwebhelper runs |
| Spotify cache | Optional | `~/.cache/spotify/Data`<br>`~/.cache/spotify/Storage` | skipped while spotify runs |
| npm cache | Optional | `~/.npm/_cacache` |  |
| Yarn cache | Optional | `~/.cache/yarn` |  |
| pip cache | Optional | `~/.cache/pip` |  |
| Gradle caches | Optional | `~/.gradle/caches` | skipped while java runs |
| Maven local repository | Optional | `~/.m2/repository` | skipped while java runs |
| Cargo download cache | Optional | `~/.cargo/registry/cache` |  |
| Go build cache | Optional | `~/.cache/go-build` |  |

`~/.cache` and `~/.config` are `%XDG_CACHE_HOME%` and `%XDG_CONFIG_HOME%`
when those are set.

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
- The only external programs it runs are Windows' own `winget` (the Windows
  Package Manager), only in the Updates tab, and `Dism.exe`, only when you
  click **Deep clean**. winget itself contacts its package sources to look
  for updates and download them; that is winget's traffic, not the app's.
- Settings, scan snapshots, and logs stay on your machine in
  `%LOCALAPPDATA%\WinFastCleaner\`. Logs contain paths and sizes only, and
  are rotated.
- Privacy cleaning only counts and removes traces. It never reads, exports,
  or logs their contents (see [Privacy tab](#privacy-tab)).

## Roadmap

| Phase | Scope | Status |
|---|---|---|
| 1 | Workspace, scanner (parallel walk and NTFS MFT), Disk tab with tree and treemap, scan snapshots | **Built** |
| 2 | Rule files, built-in rules, tiered Cleaner tab, parallel deletion, recovery bin, exclusions, safety rules, `--dry-run` | **Built** |
| 3 | Large/old file finder, duplicate finder, "What grew" | **Built** |
| 4 | Privacy cleaning, secure delete | **Built** (Windows traces; browser history and cookies deferred) |
| 5 | Startup manager, winget updater, health check, Home dashboard | **Built** (CPU/GPU temperatures deferred) |
| 6 | Scheduled cleaning, tray monitor, settings, installer, docs | **Built** |

## Using it today

### Install

Download `WinFastCleaner_<version>_x64-setup.exe` from the
[Releases](https://github.com/CharlieMcNabb1/WinFastCleaner/releases) page and
check it against `SHA256SUMS.txt` (see [Windows SmartScreen](#windows-smartscreen)).
It installs for all users into `C:\Program Files\WinFastCleaner` (one
administrator prompt), with a Start menu entry. The command-line tool `wfc.exe`
is installed next to the app.

**Uninstalling** (Settings > Apps, or the Start menu) removes the program, the
scheduled cleaning task, and the tray monitor's sign-in entry. It keeps
`%LOCALAPPDATA%\WinFastCleaner` (your settings, logs, and the **recovery bin,
which may still hold your files**): restore anything you want first, then
delete that folder yourself.

**Linux (x86-64):** download `WinFastCleaner_<version>_amd64.AppImage` (any
distro: make it executable and run it) or `WinFastCleaner_<version>_amd64.deb`
(Debian, Ubuntu: `sudo apt install ./WinFastCleaner_<version>_amd64.deb`, which
also installs `wfc`), and check it against `SHA256SUMS-linux.txt`. On Linux the
app has the Disk view, the cleaner with [Linux rules](#built-in-rules), the
file finders, What grew, and the recovery bin
(`~/.local/share/WinFastCleaner`). Privacy, Startup, Updates, Deep clean,
scheduled cleaning, and the tray are Windows-only and hidden there. Files on
other drives than your home folder can be deleted directly by green rules,
but can't go to the recovery bin yet, so yellow and red items there are
skipped.

Or build it from source as described [below](#building-from-source).

### Desktop app

- **Home** opens first: the health check, your drives' free space, how much
  junk can be cleaned (with **Clean safe items**, which opens the exact list
  of safe items to confirm), and the top three folders that grew since the
  previous scan. On every launch the app scans your system drive and looks
  for junk, with no setup wizard.
- **Disk** shows what is using space on the drive scanned at launch. Pick
  another drive or **Scan a folder…**. Click folders in the tree, treemap, or table
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
- **Files** finds your own **large and old files** (filter by size, age,
  and type: video, archives, installers, disk images, …) and
  **duplicates** (files with identical contents, compared by size and then
  by a BLAKE3 hash of the whole file). Nothing is picked for you; for
  duplicates, the last copy in a group can never be picked. Picked files go
  to the recovery bin. A search reuses the Disk tab's scan of the same
  drive, or scans first.
- **What grew** compares two scan snapshots of a drive and lists the folders
  that grew or shrank the most, such as "Discord cache +3.1 GB since Oct 9".
  Each entry opens in the Disk view, and in the Cleaner when a rule covers
  it.
- **Privacy** clears Windows traces, each on its own and none ticked by
  default (see [Privacy tab](#privacy-tab)).
- **Startup** lists what starts when you sign in: the registry Run keys
  (yours and the machine's), the Startup folders, and scheduled tasks that
  run at sign-in, each with its publisher, command, and where it comes from.
  Turning one off uses the same switch as Task Manager, so it can be turned
  back on; nothing is ever deleted. Items that start for every user need
  administrator rights to change. Services set to start automatically are
  listed separately, read-only (Windows' own services are left out).
- **Updates** lists installed apps with newer versions, using winget.
  Nothing is picked for you; tick the apps and **Update selected** runs each
  update in turn with a per-app result. If winget isn't installed, the tab
  explains how to get it (App Installer, from the Microsoft Store) instead.
- **Recover** lists recovery bin batches, newest first, with bin usage per
  drive. Restore a whole batch or chosen files, delete a batch, or empty the
  bin.
- **Settings**: theme (same as Windows, light, or dark), send safe items to
  the bin too, [scheduled cleaning](#scheduled-cleaning), the
  [tray monitor](#tray-monitor), secure delete, recovery bin limits, how long
  to keep scan snapshots, health check thresholds, your exclusions (folders,
  files, and patterns such as `*.psd`), and your custom rules folder.

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

wfc find large C:\                          # 200 largest files over 100 MB
wfc find large D:\ --min-size 1GB --kind video
wfc find old C:\Users --days 730            # unused for two years
wfc dupes D:\Photos                         # identical files (full-content hash)
wfc snapshots                               # saved scan snapshots
wfc grew C:\                                # what grew since the previous scan
wfc grew C:\ --back 5                       # ...or since five scans ago
```

`find` and `dupes` only list files; picking them for the recovery bin is
done in the app.

`wfc clean` never cleans review (red) items, always shows what it found
before acting, and refuses to run without `--yes` when there is no terminal
to ask on. Run from an elevated (administrator) terminal to use the fast MFT
scan and to clean the admin-only items. Each scan saves a small snapshot of
folder sizes (used by "What grew" in Phase 3); pass `--no-snapshot` to skip
it.

### Privacy tab

Privacy cleaning is separate from freeing space. Each item is cleared on its
own, nothing is ticked by default, and contents are never read:

| Item | What is removed | What you lose |
|---|---|---|
| Recent documents | Windows' recent files list (cleared with Windows' own "clear recent documents" call), and shortcuts left in the Recent folder | The recent files lists. The files themselves are not touched. |
| Jump lists | The jump list files in the Recent folder | Recent and pinned items in taskbar and Start jump lists |
| Run history | The values of your own `RunMRU` registry key | The Run dialog's suggestions |
| Clipboard | What is copied right now | What you copied (clipboard history, Windows key + V, is kept) |

Browser history, cookies, saved form data, and sessions aren't included yet.
Browser **caches** are cleaned in the Cleaner tab.

### Health check

The health check on Home is read-only: it never changes anything, and each
problem links to the tab where it can be fixed. Every check is green (Good),
yellow (Attention), red (Problem), or Unknown, always shown with a label.

| Check | Source | Yellow / red (defaults, change them in Settings) |
|---|---|---|
| Disk space | Each fixed drive's free space | Under 15% free / under 5% |
| Junk | The last junk scan | Over 5 GB cleanable / over 20 GB |
| Startup apps | The Startup tab | Over 10 turned on / over 20 |
| Drive health | Windows' own drive health report, plus wear and SMART sector counts when Windows lets the app read them (usually as administrator) | A warning, SSD wear over 80%, or reallocated, pending, or unreadable sectors / Windows reports the drive is failing |
| Temperatures | Not available yet: reading CPU and GPU sensors needs a hardware driver, which the app doesn't install | Always "Unknown" |
| App updates | The last check in the Updates tab | Any update available |
| Recovery bin | The recovery bin | 90% of its size limit |

### Scheduled cleaning

Off by default. In Settings, pick every day or every week, a time, and what
to clean: **safe items only** (the default), or safe and optional items
(optional items go to the recovery bin), and either every rule of those kinds
or only rules you pick. It is a Windows Task Scheduler task
(`\WinFastCleaner\Scheduled cleaning`) that runs the command-line tool
without a window (`wfcw.exe clean --scheduled`, the same as
`wfc clean --scheduled`):

- It runs as you, without administrator rights, so items that need admin are
  skipped and listed, and so are items whose app is open.
- Review (red) items are never cleaned on a schedule.
- If the PC is off at that time, it runs soon after it starts.
- Nobody is asked anything; Settings shows what the last run did, and every
  file is in the log.

### Tray monitor

Off by default. When turned on in Settings, a small separate program
(`wfc-tray.exe`, no web view) starts when you sign in and shows an icon in the
notification area. Every hour it runs the health check (and looks for junk at
most every six hours) and notifies you when a drive is low on space, when junk
piles up, or when a check turns red, each at most once a day, using the
health check thresholds from Settings. It only looks: it never cleans or
changes anything. Right-click the icon to open the app, check now, or quit
until the next sign-in.

### Secure delete

Off by default. Turn on **Offer secure delete** in Settings, and the Cleaner
and the Recover tab let you overwrite files with zeros before deleting them,
so they can't be recovered by undelete tools. It is slower, skips the
recovery bin (nothing can be restored), and is **less effective on SSDs**,
whose wear leveling may keep old copies of the data elsewhere. Files you pick
yourself (red tier) always go to the recovery bin first; delete them
securely from the Recover tab. On the command line: `wfc clean --secure`,
`wfc recover delete <BATCH> --secure`, `wfc recover empty --secure`.

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
expected for unsigned open-source software. Release builds are made by CI
(`.github/workflows/release.yml`), which also installs and uninstalls each
installer as a test, and publishes SHA-256 checksums (`SHA256SUMS.txt`) that
you can compare with `Get-FileHash .\file.exe -Algorithm SHA256`.

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

# The installer (from the repository root, after npm ci in app/)
scripts/build-installer.ps1
```

On Linux you also need the WebKitGTK development files (Debian/Ubuntu:
`sudo apt install libwebkit2gtk-4.1-dev librsvg2-dev patchelf file`), and
`scripts/build-linux.sh` builds the AppImage and `.deb` instead of the
installer.

Run the tests with `cargo test --workspace` and `npm test --prefix app`. See
[CONTRIBUTING.md](CONTRIBUTING.md) for details, including the NTFS image tests
that run on Linux.

## Project layout

| Path | What it is |
|---|---|
| `crates/core` | Rust library: scanner, MFT reader, snapshots, safety guard, rules engine, deletion, recovery bin, privacy cleaning, startup manager, winget, health check, and scheduled cleaning. No UI code. |
| `crates/cli` | `wfc` command-line tool built on `core`, and `wfcw`, the same scheduled clean without a console window (what the scheduled task runs) |
| `crates/tray` | `wfc-tray`, the tray monitor (a small Windows program, no web view) |
| `app/` | Tauri desktop app (Svelte + TypeScript frontend in `app/src`, Rust backend in `app/src-tauri`) |
| `rules/` | Built-in cleaning rule files ([format](docs/rule-format.md)) |
| `docs/` | [Architecture](docs/architecture.md), [safety model](docs/safety-model.md), and [rule file format](docs/rule-format.md) |
| `scripts/` | Developer and CI scripts: NTFS test image, building and testing the installer |

## Security

Please report vulnerabilities privately. See [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE). All code and built-in rules are written from scratch. No code
or rule files are copied from other cleaners.
