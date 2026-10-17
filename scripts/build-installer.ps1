# Builds the Windows installer (NSIS) with the CLI (wfc.exe), the scheduled
# cleaner (wfcw.exe), and the tray monitor (wfc-tray.exe) next to the app.
# Run from the repository root on Windows, after `npm ci` in app/.
# The installer ends up in target/release/bundle/nsis/.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

cargo build --release --locked -p cleaner-cli -p cleaner-tray
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

# Tauri expects extra programs named with the target triple.
$triple = (rustc -vV | Select-String '^host: (.+)$').Matches[0].Groups[1].Value
$staging = 'app/src-tauri/binaries'
New-Item -ItemType Directory -Force $staging | Out-Null
foreach ($name in 'wfc', 'wfcw', 'wfc-tray') {
    Copy-Item "target/release/$name.exe" "$staging/$name-$triple.exe" -Force
}

Push-Location app
try {
    npx --no-install tauri build --config src-tauri/tauri.bundle.conf.json
    if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }
} finally {
    Pop-Location
}

Get-ChildItem target/release/bundle/nsis/*.exe | ForEach-Object { Write-Host "Built $($_.FullName)" }
