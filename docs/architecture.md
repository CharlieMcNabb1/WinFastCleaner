# Architecture

```
┌───────────────────────────── app/ (Tauri) ─────────────────────────────┐
│  Svelte + TypeScript UI (app/src)                                      │
│        │  invoke() / Channel (only commands granted in capabilities/)  │
│        ▼                                                               │
│  Rust commands (app/src-tauri/src/commands.rs)                         │
└────────┬───────────────────────────────────────────────────────────────┘
         │                         ┌──────────── crates/cli ─────────────┐
         │                         │  `wfc` command-line tool            │
         ▼                         └──────────────┬──────────────────────┘
┌──────────────────────────── crates/core ───────────────────────────────┐
│  scan.rs      picks the MFT reader or the walker, progress, summary    │
│  mft/         NTFS Master File Table reader (read-only)                │
│  walk.rs      parallel directory walker                                │
│  tree.rs      compact in-memory folder tree                            │
│  snapshot.rs  folder-size snapshots for "What grew"                    │
│  platform.rs  drives, free space, elevation, Win32 helpers             │
│  (Phase 2+)   rules engine, deletion, recovery bin, safety checks      │
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

## Elevation

The app runs unelevated by default. The only Phase 1 feature that needs
admin rights is the fast MFT scan. The Disk tab says so and offers
**Restart as administrator**, which relaunches the app through UAC (one
prompt for the session, as the spec allows). Without admin, the walker is
used, so nothing breaks.

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
  native folder picker (`dialog:allow-open`). There are no shell, fs, http,
  or opener permissions. (The dialog plugin depends on the fs plugin, but
  none of its permissions are granted.)
- **CSP:** `default-src 'self'`, with no inline scripts or styles, no remote
  origins, and `object-src`, `frame-src`, `worker-src`, `base-uri`, and
  `form-action` set to none. Prototypes are frozen.
- **No raw paths for actions:** the UI refers to items by `(scanId, nodeId)`.
  The backend resolves paths from its own tree. The only path the UI sends
  is the folder to scan, which is validated (absolute, no device paths, no
  NUL, length-limited).
- **No network:** `deny.toml` bans HTTP and TLS client crates.
- WebView2 keeps its own browser data folder
  (`%LOCALAPPDATA%\io.github.winfastcleaner\`). The app loads no remote
  content, so this only holds WebView2's own caches.

## Testing

| What | Where | Runs on |
|---|---|---|
| Unit tests (tree, walker, MFT parsing, snapshots, formatting) | `crates/*/src` | Windows + Linux CI |
| MFT reader vs. walker on a real NTFS image (hard links, attribute lists, sparse files, ADS, junctions, symlinks, Unicode) | `crates/core/tests/ntfs_image.rs` + `scripts/ntfs-test-image.sh` | Linux CI |
| MFT scan vs. walker on a real Windows folder, plus a full system-drive MFT scan | `crates/core/tests/windows_volume.rs` | Windows CI (elevated) |
| CLI end-to-end | `crates/cli/tests/cli.rs` | Windows + Linux CI |
| Frontend formatting and treemap layout | `app/src/lib/*.test.ts` | Windows CI |
| UI in a browser with fake data | `npm run dev:mock` | Local |
