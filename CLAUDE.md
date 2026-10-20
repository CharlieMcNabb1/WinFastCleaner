# WinFastCleaner

## Layout
- `crates/core` – Rust core: scanning, cleaning, safety guard, recovery bin
- `crates/cli` – `wfc` CLI
- `crates/tray` – tray app
- `app/` – Tauri + Svelte/TS desktop app
- `rules/` – cleaning rule definitions (TOML); format in `docs/rule-format.md`
- `docs/` – architecture and safety model
- `scripts/` – installer build/test, NTFS test image

## Commands
- Rust tests: `cargo test --workspace`
- Lint: `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`
- UI tests: `npm test --prefix app`
- UI only, fake data: `cd app; npm run dev:mock`

## Rules
- Never weaken safety rules in `crates/core` (protected paths, tiers, recovery bin). See `docs/safety-model.md`.
- Never run the app or `wfc` against the real system while developing; use tests and `dev:mock`.
- Run the relevant tests before saying a change is done.
- Small, focused changes; one concern per branch.
- UI palette: monochrome greys `#FAFAF8` `#DADADA` `#B0B3B8` `#43464B` `#0F1012` (see `docs/design.md`). The primary action is near-black (near-white in dark). Green/yellow/red are reserved for safety tiers and are the only colors in the UI.
