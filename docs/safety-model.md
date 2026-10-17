# Safety model

WinFastCleaner deletes files, so a single bug can destroy user data. These
rules are **non-negotiable**. Every deletion path enforces them in the
`core` library (`crates/core`), not only in the UI, so the CLI, scheduled
cleaning, the desktop app, and any future elevated helper all get the same
protection.

| # | Rule | Where it is enforced |
|---|---|---|
| 1 | **Protected paths blocklist.** Never delete inside `C:\Windows` (except temp, log, and update-cache subfolders that rules list), `C:\Program Files`, `C:\Program Files (x86)`, `C:\ProgramData` (except rule-listed caches), the root of any drive, a user profile root, `WinSxS`, `System32`, boot and recovery folders, or the app's own files. Checked on the **final resolved path**, after expanding variables, symlinks, and junctions. | `safety::Guard`, called by the rules loader, the junk scan, the plan builder, and again by the executor on the path the open file handle reports. |
| 2 | **No following links.** Never follow symlinks, junctions, or mount points while finding or deleting files. | `rules::expand_path`, `cleaner::scan`, `fsops::open` (opens the link itself, never its target). |
| 3 | **Rules define green and yellow.** Only files matched by a rule can be green or yellow. Anything picked by hand is red. | `rules` (a rule can't be red), `cleaner::plan_manual` (always red). |
| 4 | **Red is never automatic.** Red items are never pre-selected, never part of scheduled cleaning, and always go to the recovery bin. | `cleaner::execute` (only green may skip the bin), the UI's defaults, `wfc clean` (rules only). |
| 5 | **Exclusions always win** over every rule and selection. | `settings::Exclusions`, checked by the scan, the plan builder, the executor, and empty-folder removal. |
| 6 | **Dry run first.** Every clean computes the exact file list and size first, and that list is what gets acted on. | `cleaner::plan_*` then `cleaner::execute`. The app only executes a plan the user previewed, once. `wfc clean --dry-run` prints the list. |
| 7 | **Fail safe.** Locked, in-use, or permission-denied files are skipped and reported. One failure never aborts the batch or corrupts the recovery index. | `cleaner::execute` (per-file outcomes), `recovery` (write-ahead index). |
| 8 | **Running apps.** If a rule needs its app closed, it is skipped and the user is told. Nothing is force-closed. | `cleaner::scan` (rule status) and `cleaner::execute` (checked again right before acting). |
| 9 | **Logging.** Every action is logged locally (path, size, tier, result) and never sent anywhere. | `log` (`logs\cleanup.log`, rotated at 5 MB, 3 old files kept). |
| 10 | **Tests.** Deletion and recovery are tested in temporary folders, including protected paths, links, and exclusions. | Unit tests in `cleaner`, `recovery`, `safety`, `fsops`, `rules`, `settings`; end-to-end tests in `crates/cli/tests`. |

## What the guard protects

`safety::Guard` decides whether a path may be deleted, moved, or restored
into. It works on a normalized path: case is ignored, `/` and `\` are the
same, and anything ambiguous is refused outright (`..`, UNC and device paths
such as `\\server\share` or `\\.\`, alternate data streams `file:stream`,
names ending in a dot or space, and 8.3 short names like `PROGRA~1`).

Always protected (nothing inside them is ever touched):

- the Windows folder (`C:\Windows`), except these junk folders, which rules
  may clean: `Temp`, `Logs`, `Minidump`, `LiveKernelReports`,
  `SoftwareDistribution\Download`, the Delivery Optimization cache, and the
  `MEMORY.DMP` file;
- `Program Files`, `Program Files (x86)`;
- `ProgramData`, except `Microsoft\Windows\WER\ReportArchive`,
  `ReportQueue`, and `Temp` (error reports);
- these names wherever they appear: `WinSxS`, `System32`, `SysWOW64`,
  `System Volume Information`, `$Recycle.Bin`, `$WinREAgent`,
  `$Windows.~BT`, `$Windows.~WS`, `$SysReset`, `$GetCurrent`,
  `WindowsApps`, `WpSystem`, `wuDownloadCache`, and the recovery bin itself;
- at the top of any drive: `Boot`, `EFI`, `Recovery`, `Config.Msi`,
  `Documents and Settings`, `PerfLogs`, `Windows.old`, `MSOCache`, and the
  files `bootmgr`, `BOOTNXT`, `pagefile.sys`, `hiberfil.sys`,
  `swapfile.sys`, `DumpStack.log`, and other boot files;
- the `Default`, `Default User`, and `All Users` profiles;
- inside every user profile: Windows' encryption keys
  (`AppData\Roaming\Microsoft\Protect` and `Crypto`), saved credentials
  (`Credentials`, `Vault`), certificates (`SystemCertificates`), Microsoft
  Store app data (`AppData\Local\Packages`, `WindowsApps`), and the registry
  hives (`NTUSER.DAT*`, `UsrClass.dat*`);
- the app's own data folder and program file (and its install folder when it
  is named `WinFastCleaner`, so a portable copy in Downloads doesn't lock
  Downloads).

Never removed themselves (their contents may be cleaned): the root of every
drive, the `Users` folder, `Users\Public`, and each user profile folder.
Empty-folder removal also never removes a folder that contains a protected
location or an exclusion.

Protected files are **filtered out when the cleaner scans**, so they never
appear in results and can't be selected by accident. The Disk view still
shows them (sizes must add up), with a lock icon and "Send to cleaner"
disabled.

## How a clean works

1. **Find.** Each rule's paths are expanded (variables, `*` folder
   patterns), never through links. Each location's real path is resolved and
   checked by the guard. Files are matched by the rule's patterns and age
   filter. Links, system files, cloud-only files, protected files, and
   excluded files are left out (and counted, so the UI can say so).
2. **Plan.** The user's selection becomes an exact list of files with their
   sizes and modified times. This is the `--dry-run` output, and the app
   shows its totals before asking.
3. **Execute.** For each file, in parallel:
   - open it **without following links** (`FILE_FLAG_OPEN_REPARSE_POINT`,
     with `DELETE` access);
   - through that handle, check it is still a plain file (not a folder, link,
     system file, or cloud file) with the same size and modified time as in
     the preview;
   - ask Windows for the handle's **final path**, and require it to be the
     planned path and to pass the guard and the exclusions;
   - delete it or rename it into the recovery bin **through the same
     handle**, so nothing can swap the file between the check and the act.

   Anything that fails a check, is in use, or is denied is skipped with a
   reason. Afterwards, folders left empty are removed (deepest first, never
   above the rule's location, each one re-checked), the bin's limits are
   applied, and every outcome is logged.

## File finders

The large/old file finder and the duplicate finder only list files. Their
results are always red tier: nothing is picked for the user, and picked
files are moved to the recovery bin through the same plan and checks as any
other hand-picked item. Protected and excluded files are never listed.

For duplicates, the core refuses a plan that would remove every copy in a
group, or whose kept copy no longer exists; the UI also never lets the last
copy be picked. Hashing opens files read-only without following links, and
skips cloud placeholders so it never downloads them.

## Privacy cleaning

The Privacy tab clears Windows traces (recent documents, jump lists, Run
history, clipboard). Each is cleared only when the user ticks it; none is
ticked by default. Contents are never read: items are only counted, and the
log records file paths and sizes, or for the Run history and clipboard just
"cleared N entries" / "emptied". Recent-document shortcuts and jump list files
are deleted through the same link-safe handles and guard as everything else.
The only registry data touched is the values of the user's own
`HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU`.

## Secure delete

Secure delete (off by default, offered only when turned on in Settings)
overwrites a file with zeros through the same link-safe handle, flushes it to
disk, and then deletes it. It is only offered where it can't conflict with
the rules above:

- for rule-based (green and yellow) cleaning, where those files skip the
  recovery bin;
- for deleting batches from the recovery bin.

**Red items always go to the recovery bin first** (rule 4), even when secure
delete is chosen; the core enforces this, not just the UI. On SSDs, wear
leveling may keep old copies of the data, so the UI says it is less
effective there.

## Recovery bin

Yellow and red items (and green ones if the user asks) are **moved**, never
copied, so a batch is instant:

- **Drive of the app's data folder (normally C:).** The bin is inside the
  data folder: `%LOCALAPPDATA%\WinFastCleaner\recovery\bin`. It is private
  to the user like the rest of the profile, and protected by the guard.
- **Other drives.** `X:\.winfastcleaner-recovery\<user>`, created hidden.
  The per-user folder gets explicit permissions (the user, SYSTEM, and
  administrators only; nothing inherited from the drive root), and a bin
  folder owned by anyone else, or that is a link, is refused. FAT and exFAT
  drives have no permissions at all, so there the bin is as private as the
  files it holds were.

The spec suggests `X:\.<appname>-recovery\` on every drive. On the system
drive that folder would inherit permissions from `C:\` that let other
accounts on the PC list it, so the bin lives in the private data folder
instead (still the same drive, still an instant rename).

The index (`recovery\index.json` in the data folder) records each file's
original path, size, tier, and batch. A batch is written as *pending*
**before** any file moves, and on every load pending items are reconciled
with what is actually in the bin, so a crash at any point leaves a correct
index. The index is locked against other processes (such as a scheduled
cleanup) while it changes.

**Restore** moves files back through the same link-safe handle. The source
must still be inside the bin, the target must pass the guard, and the target
folder is recreated if needed but refused if it is now a link. When the
original path is taken, the user chooses: skip, keep both (`name (restored)`),
or overwrite (the existing file first goes to the bin, so that is undoable
too).

**Eviction.** When a batch pushes the bin over its limits (default: 5 GB or
10% of the drive, whichever is smaller; 5 batches; 14 days), the oldest
batches are deleted for good. The newest batch is always kept.

## Scanner safety

The scanner is strictly **read-only**:

- The parallel walker never descends into *name-surrogate* reparse points
  (symlinks, junctions, mount points). It does descend into non-surrogate
  reparse points such as OneDrive cloud folders, because those are the real
  directory and not a link to somewhere else. Their files are never opened,
  so cloud placeholders are never downloaded.
- The MFT reader opens the volume read-only and only parses metadata. It
  builds the folder tree from parent references, so links can't cause loops
  or double counting.
- Files that can't be read (permission denied, locked) are counted as
  skipped and reported. They never abort a scan.

## Elevation

The UI runs unelevated by default. Admin rights are needed for the MFT fast
scan, for rules marked `requires_admin` (Windows' own temp, error reports,
crash dumps, update caches, and logs), and for Deep clean. When the app isn't
elevated it says which items need admin and offers **Restart as
administrator** (one UAC prompt for the rest of the session). Without admin
rights those items are shown but skipped, with the reason.

If an elevated helper process is added later, it must accept only a fixed
set of validated commands, re-check every path against these rules itself,
and never run arbitrary commands.

## Deep clean

Deep clean runs Windows' own component store cleanup,
`%SystemRoot%\System32\Dism.exe /Online /Cleanup-Image /StartComponentCleanup`,
with fixed arguments (nothing from the UI is passed to it). It is a separate,
clearly labeled action because it is slow, and it never runs as part of
normal cleaning. It needs administrator rights. Windows does the work itself
and finishes it safely, so it isn't cancelled midway.

## App hardening

- **Tauri:** only the app's own commands are exposed, each granted one by
  one in `app/src-tauri/capabilities/main.json`. The only plugin is the
  native file and folder picker. There are no shell, fs, http, or opener
  plugins. A strict Content Security Policy is set, the window never loads
  remote content, and prototypes are frozen.
- **The frontend never sends paths for actions.** It refers to Disk view
  items by scan node ID and to cleaner files by their index in the
  backend's own junk scan. The backend builds the plan, keeps it, and runs
  only that previewed plan, once. The only paths the UI sends are the folder
  to scan and exclusions (a list that can only protect files).
- **No network access** from the app. `deny.toml` bans HTTP and TLS client
  crates.
- **Rule files are data only.** Loading a rule can never execute code or run
  a command. Unknown fields are rejected, every value is validated, and a
  rule aimed at a protected location is refused (see the
  [rule format](rule-format.md)).
