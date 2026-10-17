# Cleaning rule format

Everything the cleaner can find is defined by **rule files**: plain TOML
files, one per app or area. The built-in rules live in [`rules/`](../rules/)
and are compiled into the app. You can add your own in your rules folder:

```
%LOCALAPPDATA%\WinFastCleaner\rules\*.toml
```

(**Settings → Custom cleaning rules → Open rules folder** opens it.) No Rust
code is needed to add a rule.

Rules are **data only**. Loading a rule can never run code or a command.
Unknown fields are rejected, every value is checked, and a rule that points
into a protected location (see the [safety model](safety-model.md)) is
refused. Invalid rules are **reported** (in the Cleaner tab, in Settings,
and by `wfc rules list`), never silently skipped.

## Example

```toml
[[rule]]
id = "exampleapp.cache"
name = "Example App cache"
group = "Apps"
tier = "green"
description = "Images and pages Example App saved to open faster."
lose = "Nothing. Example App downloads them again when needed."
paths = [
    '%LOCALAPPDATA%\ExampleApp\Cache',
    '%LOCALAPPDATA%\ExampleApp\Profiles\*\GPUCache',
]
exclude = ['index', 'keep/**']
min_age_days = 2
close_apps = ["exampleapp.exe"]
```

Use single quotes (TOML literal strings) for paths, so backslashes don't need
escaping.

## Fields

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | Unique id, 3 to 64 characters of `a-z`, `0-9`, `.` and `-`, starting with a letter or digit. Convention: `app.what`, such as `chrome.cache`. A rule whose id is already taken is reported and not used. |
| `name` | yes | Short name shown in the Cleaner tab (up to 80 characters). |
| `group` | yes | The app or area it belongs to, such as `Windows`, `Google Chrome`, or `Developer tools` (up to 40 characters). Results are grouped by it. |
| `tier` | yes | `"green"` or `"yellow"` (see below). Rules can never be `"red"`: red is only for files you pick yourself. |
| `description` | yes | One sentence on what these files are (up to 400 characters). |
| `lose` | yes | What you lose by deleting them (up to 300 characters). |
| `paths` | yes | 1 to 16 locations to clean (see [Paths](#paths)). |
| `include` | no | Patterns of files to clean, relative to each location. Default: every file (`**`). |
| `exclude` | no | Patterns of files to leave alone, relative to each location. Exclude wins over include. |
| `min_age_days` | no | Only clean files last modified more than this many days ago (0 to 3650). |
| `requires_admin` | no | `true` if the files can only be removed with administrator rights. Without them, the rule is shown but skipped, with the reason. Default `false`. |
| `close_apps` | no | Program file names, such as `["chrome.exe"]`. While any of them runs, the rule is skipped and you are told to close it. Nothing is ever force-closed. |
| `remove_empty_folders` | no | Remove folders inside each location that are left empty after cleaning. The location itself is always kept. Default `true`. |

Text fields can't be empty or contain control characters.

### Tiers

| Tier | Use it for | Selected by default | When cleaned |
|---|---|---|---|
| `green` (Safe) | Files that regenerate automatically with no impact: temp files, caches, crash dumps, thumbnail cache. | Yes | Deleted directly, or moved to the recovery bin if the user turned that on. |
| `yellow` (Optional) | Safe, but with a cost, such as a re-download or a slower next start: download caches, shader caches, package manager caches, old logs. | No | Moved to the recovery bin. |

When in doubt, choose `yellow`.

### Paths

Every path starts with one of these variables, then names folders inside it:

| Variable | Usually |
|---|---|
| `%TEMP%` | `C:\Users\you\AppData\Local\Temp` |
| `%LOCALAPPDATA%` | `C:\Users\you\AppData\Local` |
| `%APPDATA%` | `C:\Users\you\AppData\Roaming` |
| `%USERPROFILE%` | `C:\Users\you` |
| `%PROGRAMDATA%` | `C:\ProgramData` |
| `%SYSTEMROOT%` | `C:\Windows` |

Rules for paths:

- Only `%TEMP%` may be used on its own. Every other variable must be followed
  by at least one folder name, so a rule can't target a whole profile.
- A path segment may use `*` (any characters) and `?` (one character) to
  match folder names, for example `User Data\*\Cache` for every browser
  profile. `**`, `.`, `..`, `:`, and `%` are not allowed in segments, and a
  name can't end with a dot or a space.
- A path may name a single file (for example `%SYSTEMROOT%\MEMORY.DMP`).
- Folders that don't exist are ignored. Links (symlinks and junctions) are
  never followed, neither while finding locations nor while cleaning.
- Locations under `%SYSTEMROOT%` and `%PROGRAMDATA%` are protected except for
  a short list of junk folders (temp, logs, crash dumps, error reports,
  Windows Update and Delivery Optimization caches). A rule pointing anywhere
  else in them is refused when it loads.

`%TEMP%` is only used when its folder name looks like a temp folder (it
contains `temp` or `tmp`). If `TEMP` is set to something else, such as your
Documents folder, `%TEMP%` rules find nothing.

### Patterns (`include` and `exclude`)

Patterns are matched against each file's path **relative to the location**,
ignoring case:

| Pattern | Matches |
|---|---|
| `*.log` | `.log` files directly in the location |
| `**/*.log` | `.log` files at any depth |
| `thumbcache_*.db` | `thumbcache_1024.db`, `thumbcache_idx.db`, … |
| `keep/**` | everything inside the `keep` folder |
| `f?le.txt` | `file.txt`, `fole.txt`, … |

`*` matches within one name, `?` matches one character, and `**` as a whole
segment matches any number of folders (including none). `/` and `\` both
separate folders. There are no character classes or braces. At most 64
patterns per field.

## What is never cleaned, whatever a rule says

Rules only **find** candidates. Each file then goes through the same checks
as everything else the cleaner touches:

- protected locations (Windows, Program Files, ProgramData, drive roots,
  profile folders, credentials and app packages in your profile, the app's
  own files, and more; see the [safety model](safety-model.md)),
- your exclusions list,
- links, system files, and cloud-only files (OneDrive placeholders),

and is checked again, through the file itself, right before it is deleted
or moved. Files left out this way are never shown or cleaned; the Cleaner
tab says how many were left out.

## Checking your rules

```powershell
wfc rules check .\my-rules.toml       # report problems in a rule file
wfc rules list                        # every rule the app will use
wfc clean --rules-dir .\my-rules --tier yellow --dry-run --list
```

`--rules-dir` uses only the rules in that folder (no built-in rules), and
`--dry-run` lists exactly what would be cleaned without changing anything.

## Contributing a rule

1. Add the rule to the right file in [`rules/`](../rules/) (or a new file
   named after the app or area, added to `BUILTIN_FILES` in
   `crates/core/src/rules.rs`).
2. Write the rule yourself from what you know about the app. Don't copy rule
   files from other cleaners: they may be under licenses that aren't
   compatible with this project's.
3. Run `cargo test -p cleaner-core` (a test loads every built-in rule) and
   try it with `--dry-run` on a real machine.
4. In the pull request, explain why the files are safe to delete and how the
   app behaves afterwards.
