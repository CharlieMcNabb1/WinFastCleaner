# Installs the NSIS installer silently, checks the installed programs work,
# then uninstalls and checks everything WinFastCleaner added is gone while
# the user's data folder is kept. For CI (needs administrator rights, as the
# installer installs for all users).
#
#   scripts/test-installer.ps1 target/release/bundle/nsis/WinFastCleaner_x.y.z_x64-setup.exe

param([Parameter(Mandatory = $true)][string]$Installer)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Check([bool]$ok, [string]$what) {
    if (-not $ok) { throw "FAILED: $what" }
    Write-Host "ok: $what"
}

$uninstallKey = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\WinFastCleaner'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$runValue = 'WinFastCleaner tray monitor'
$dataDir = Join-Path $env:LOCALAPPDATA 'WinFastCleaner'

# A marker in the data folder, which the uninstaller must leave alone.
New-Item -ItemType Directory -Force $dataDir | Out-Null
$marker = Join-Path $dataDir 'installer-test-marker.txt'
Set-Content $marker 'keep me'

# --- install
$p = Start-Process -FilePath $Installer -ArgumentList '/S' -Wait -PassThru
Check ($p.ExitCode -eq 0) "installer exit code ($($p.ExitCode))"
Check (Test-Path $uninstallKey) 'uninstall entry registered'
$dir = Join-Path ${env:ProgramFiles} 'WinFastCleaner'
foreach ($f in 'WinFastCleaner.exe', 'wfc.exe', 'wfcw.exe', 'wfc-tray.exe', 'uninstall.exe') {
    Check (Test-Path (Join-Path $dir $f)) "installed $f"
}
Check (-not (Test-Path (Join-Path $env:LOCALAPPDATA 'WinFastCleaner\WinFastCleaner.exe'))) 'program files are not in the data folder'

$version = & (Join-Path $dir 'wfc.exe') --version
Check ($LASTEXITCODE -eq 0 -and $version -match '^wfc \d+\.\d+\.\d+') "wfc --version ($version)"
& (Join-Path $dir 'wfc.exe') drives | Out-Null
Check ($LASTEXITCODE -eq 0) 'wfc drives'
$p = Start-Process -FilePath (Join-Path $dir 'wfcw.exe') -ArgumentList 'something', 'else' -Wait -PassThru
Check ($p.ExitCode -eq 2) 'wfcw refuses anything but the scheduled clean'

# --- what the app adds when the user turns things on (simulated here)
$task = '\WinFastCleaner\Scheduled cleaning'
schtasks /Create /TN $task /TR "`"$dir\wfcw.exe`" clean --scheduled" /SC WEEKLY /D SUN /ST 12:00 /F | Out-Null
Check ($LASTEXITCODE -eq 0) 'test task created'
New-ItemProperty -Path $runKey -Name $runValue -Value "`"$dir\wfc-tray.exe`"" -PropertyType String -Force | Out-Null

# --- uninstall
$uninstaller = Join-Path $dir 'uninstall.exe'
Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait | Out-Null
# NSIS uninstallers finish from a temporary copy; wait for it.
$deadline = (Get-Date).AddSeconds(120)
while ((Test-Path (Join-Path $dir 'WinFastCleaner.exe')) -and (Get-Date) -lt $deadline) { Start-Sleep -Seconds 1 }
Start-Sleep -Seconds 2

foreach ($f in 'WinFastCleaner.exe', 'wfc.exe', 'wfcw.exe', 'wfc-tray.exe', 'uninstall.exe') {
    Check (-not (Test-Path (Join-Path $dir $f))) "removed $f"
}
Check (-not (Test-Path $uninstallKey)) 'uninstall entry removed'
schtasks /Query /TN $task 2>$null | Out-Null
Check ($LASTEXITCODE -ne 0) 'scheduled cleaning task removed'
Check ($null -eq (Get-ItemProperty -Path $runKey -Name $runValue -ErrorAction SilentlyContinue)) 'tray sign-in entry removed'
Check ((Get-Content $marker) -eq 'keep me') 'data folder kept'
Remove-Item $marker
Write-Host 'Installer test passed.'
# The last native command (schtasks /Query, expected to fail) must not
# become the script's exit code.
exit 0
