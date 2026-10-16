# Safety model

WinFastCleaner deletes files, so a single bug can destroy user data. These
rules are **non-negotiable**. Every deletion path must enforce them in the
`core` library (`crates/core`), not only in the UI, so the CLI, scheduled
cleaning, the desktop app, and any future elevated helper all get the same
protection.

| # | Rule | Where it is enforced | Status |
|---|---|---|---|
| 1 | **Protected paths blocklist.** Never delete inside `C:\Windows` (except temp, log, and update-cache subfolders explicitly listed by rules), `C:\Program Files`, `C:\Program Files (x86)`, `C:\ProgramData` (except rule-listed caches), the root of any drive, the user profile root, `WinSxS`, `System32`, boot and recovery partitions, or the app's own install folder. Checked on the **final resolved path**, after expanding environment variables, symlinks, and junctions. | `core` deletion planner and executor | Phase 2 |
| 2 | **No following links.** Never follow symlinks, junctions, or mount points during deletion. Delete the link itself only if a rule targets it. | `core` deletion executor | Phase 2. The scanner already never follows links (Phase 1). |
| 3 | **Rules define green and yellow.** Only paths matched by a rule file can be green or yellow. Anything selected by hand (disk view, large/old finder, duplicate finder) is always red. | `core` rules engine | Phase 2 |
| 4 | **Red is never automatic.** Red items are never pre-selected, never included in scheduled cleaning, and always go through the recovery bin. | `core` plan builder, CLI scheduled mode | Phase 2 |
| 5 | **Exclusions always win** over every rule and selection. | `core` plan builder and executor | Phase 2 |
| 6 | **Dry run first.** Every clean computes the exact file list and total size before deleting, and that list is exactly what gets deleted. The CLI has `--dry-run`. | `core` plan/execute split | Phase 2 |
| 7 | **Fail safe.** Locked, in-use, or permission-denied files are skipped and reported. A failure on one file never aborts the batch or leaves the recovery index inconsistent. | `core` executor, recovery index | Phase 2 |
| 8 | **Running apps.** If a rule needs its app closed (for example a browser), the rule is skipped and the user is told. Nothing is force-closed. | `core` rules engine | Phase 2 |
| 9 | **Logging.** Every deletion batch is logged locally (paths, sizes, tier, result). Logs are never sent anywhere and are rotated. | `core` logging | Phase 2 |
| 10 | **Tests.** Deletion and recovery code has automated tests that run in temporary directories, including tests confirming that protected paths, links, and exclusions are never deleted. | `crates/core/tests` | Phase 2 |

## Scanner safety (Phase 1)

The scanner is strictly **read-only**:

- The parallel walker never descends into *name-surrogate* reparse points
  (symlinks, junctions, mount points). It does descend into non-surrogate
  reparse points such as OneDrive cloud folders, because those are the real
  directory and not a link to somewhere else. Their files are never opened,
  so cloud placeholders are never downloaded.
- The MFT reader opens the volume read-only and only parses metadata. It
  builds the folder tree from parent references, so links cannot cause loops
  or double counting.
- Files that can't be read (permission denied, locked) are counted as
  skipped and reported. They never abort a scan.

## Elevation

The UI runs unelevated by default. The only Phase 1 operation that benefits
from admin rights is the MFT fast scan. When it isn't elevated, the app says
so in the Disk tab and offers **Restart as administrator** (one UAC prompt
for the rest of the session). Without admin rights the parallel walker is
used, so nothing breaks.

If an elevated helper process is added later, it must accept only a fixed
set of validated commands, re-check every path against these rules itself,
and never run arbitrary commands.

## App hardening

- **Tauri:** only the app's own commands are exposed, through an explicit
  capability file. There are no shell, fs, http, or opener plugins. A strict
  Content Security Policy is set, the window never loads remote content, and
  prototypes are frozen.
- **The frontend never sends raw paths** to the backend for actions. It
  refers to scan nodes by ID, and the backend resolves the path from its own
  scan tree.
- **No network access** from the app. `deny.toml` bans HTTP and TLS client
  crates.
- **Rule files are data only** (Phase 2). Loading a rule can never execute
  code or run a command, and every rule is validated against a schema.
