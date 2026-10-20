# Contributing to WinFastCleaner

Thanks for helping! This app deletes files, so a few rules matter more here
than in most projects. Please read the ground rules before opening a PR.

## Ground rules

1. **Never weaken a safety rule** to make something work. The rules are
   listed in [docs/safety-model.md](docs/safety-model.md). If a rule seems
   wrong, open an issue that explains the trade-off.
2. **Test deletion only inside temporary directories.** Tests create
   synthetic files with `tempfile` and never touch real user folders.
3. **Never commit real user data.** That means no logs, scan results,
   screenshots, or fixtures containing real usernames, file paths, or file
   names. Use synthetic names such as `C:\Users\test-user\...`.
4. **Write everything from scratch.** Don't copy code or rule files from
   BleachBit (GPLv3), CCleaner, or any other cleaner. Ideas are fine, but text
   and code are not.
5. **Dependencies must be permissively licensed** and well maintained.
   `cargo deny check` enforces the license allowlist in `deny.toml`. Explain
   any new dependency in your PR.
6. **No network access, telemetry, or remote content.** The `deny.toml` ban
   list blocks HTTP and TLS client crates on purpose.
7. **No new features outside the [roadmap](README.md#roadmap)** without an
   issue first.

## AI-assisted pull requests

PRs written with AI tools (Claude Code, Copilot, and so on) are welcome, on
the same terms as any other PR:

- **You are the author.** Read and understand every line before you open the
  PR, and be ready to explain it in review. "The AI wrote it" is not an
  answer.
- **Tests must pass.** Run the checks below yourself; CI must be green before
  review.
- The ground rules above still apply, especially rule 1: an AI tool must
  never weaken a safety rule to make something work.

## Development setup

- Rust: install with [rustup](https://rustup.rs). The toolchain version is
  pinned in `rust-toolchain.toml` and installs automatically.
- Node.js 22 and npm, for the frontend in `app/`.
- On Windows: Microsoft C++ Build Tools and WebView2.
- On Linux (core/CLI development and tests only): the usual build tools, plus
  `ntfs-3g` for the NTFS image tests.

### Common commands

```sh
cargo fmt --all                               # format
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                        # all Rust tests
cargo deny check                              # licenses + advisories

cd app
npm ci --ignore-scripts
npm run check                                 # svelte-check / TypeScript
npm test                                      # frontend unit tests
npm run tauri dev                             # run the desktop app
```

### The installer (Windows)

```powershell
cd app; npm ci --ignore-scripts; cd ..
scripts/build-installer.ps1        # builds wfc, wfcw, wfc-tray, and the NSIS installer
scripts/test-installer.ps1 (Get-ChildItem target/release/bundle/nsis/*-setup.exe).FullName
```

`test-installer.ps1` installs for all users (run it from an elevated
PowerShell, ideally in a throwaway VM), checks the programs, and uninstalls.

### Releases

Bump the version in `Cargo.toml` (workspace), `app/package.json`, and
`app/src-tauri/tauri.conf.json`, merge, then push a tag such as `v0.2.0`.
`release.yml` builds and tests the installer and creates a **draft** GitHub
release with SHA-256 checksums for a maintainer to review and publish.

### NTFS (MFT reader) tests on Linux

The MFT reader is tested against a real NTFS image built with `mkntfs`. Root
is required to mount the image while it is filled with synthetic files:

```sh
sudo scripts/ntfs-test-image.sh setup /tmp/wfc-ntfs
WFC_NTFS_TEST_IMAGE=/tmp/wfc-ntfs/test.img \
WFC_NTFS_TEST_MOUNT=/tmp/wfc-ntfs/mnt \
    cargo test -p cleaner-core --test ntfs_image
sudo scripts/ntfs-test-image.sh teardown /tmp/wfc-ntfs
```

Without these environment variables, the image tests are skipped (with a
message). CI always runs them.

### Optional pre-commit secret scan

If you have [gitleaks](https://github.com/gitleaks/gitleaks) installed:

```sh
git config core.hooksPath .githooks
```

## Adding a cleaning rule

Adding support for a new app needs no Rust code. The format is documented in
[docs/rule-format.md](docs/rule-format.md).

1. Add the rule to the matching file in `rules/` (or create
   `rules/<app-or-area>.toml` and add it to `BUILTIN_FILES` in
   `crates/core/src/rules.rs`).
2. Pick the safety tier honestly. Green means it regenerates automatically
   and the user loses nothing. When in doubt, use yellow.
3. Write the `description` and `lose` ("what you lose") text in plain
   language.
4. Add the rule to the **Built-in rules** table in `README.md` (a test checks
   that every rule is listed).
5. Check it: `wfc rules check rules/<file>.toml`, `cargo test -p cleaner-core`,
   and on a real machine `wfc clean --rule <id> --dry-run --list`.
6. Open a PR that explains where the files come from and why the tier is
   right.

Rule files are data only. They can never run commands or target protected
paths, and the loader rejects any that try.

## Pull request checklist

- [ ] `cargo fmt`, `cargo clippy -D warnings`, and `cargo test` pass.
- [ ] Frontend `npm run check` and `npm test` pass (if you touched `app/`).
- [ ] New deletion or recovery code has tests that run in temp directories,
      including protected-path, link, and exclusion cases. CLI tests that
      clean pass `--rules-dir` with test rules, so built-in rules never run.
- [ ] No real user data, secrets, or copied third-party rules.
- [ ] Docs updated if behavior changed.
