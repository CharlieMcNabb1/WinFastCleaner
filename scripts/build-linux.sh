#!/usr/bin/env bash
# Builds the Linux packages (AppImage and .deb) with the CLI (wfc) next to
# the app. Scheduled cleaning and the tray are Windows-only, so wfcw and
# wfc-tray are not included. Run from the repository root on Linux, after
# `npm ci` in app/. The packages end up in target/release/bundle/.

set -euo pipefail

cargo build --release --locked -p cleaner-cli --bin wfc

# Tauri expects extra programs named with the target triple.
triple=$(rustc -vV | sed -n 's/^host: //p')
mkdir -p app/src-tauri/binaries
cp target/release/wfc "app/src-tauri/binaries/wfc-$triple"

(
    cd app
    npx --no-install tauri build \
        --config src-tauri/tauri.bundle.conf.json \
        --config src-tauri/tauri.linux-bundle.conf.json
)

ls target/release/bundle/appimage/*.AppImage target/release/bundle/deb/*.deb
