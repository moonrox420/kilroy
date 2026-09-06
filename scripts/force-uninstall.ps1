<#
.SYNOPSIS
  Force-uninstall Kilroy when ghost processes are holding file locks.

.DESCRIPTION
  Aggressively hunts down Kilroy.exe AND its WebView2 helper children (which
  don't show up under Get-Process -Name 'Kilroy' because they're named
  msedgewebview2.exe). Uses CIM to match by ExecutablePath + CommandLine so
  helpers spawned by Tauri get caught.

  Then calls the regular uninstall.ps1 to remove files / shortcuts / registry.
  Anything still locked at the end is scheduled for deletion on next reboot
  via MoveFileEx(MOVEFILE_DELAY_UNTIL_REBOOT).

.PARAMETER IncludeBuild
  Also delete build artifacts in current repo (passed through to uninstall.ps1).

.PARAMETER IncludeProjectData
  Also delete .kilroy/ in current dir (passed through to uninstall.ps1).

.PARAMETER NoReboot
  Skip the delete-on-reboot fallback. Exit non-zero if anything is still locked.

.EXAMPLE
  .\scripts\force-uninstall.ps1
  Standard force-uninstall -- kills ghosts, removes everything Kilroy installed.

.EXAMPLE
  .\scripts\force-uninstall.ps1 -IncludeBuild -IncludeProjectData
  Nuclear force-uninstall.
#>

[CmdletBinding()]
param(
  [switch]$IncludeBuild,
  [switch]$IncludeProjectData,
  [switch]$NoReboot
)

$ErrorActionPreference = 'Continue'

function Find-KilroyProcesses {
  Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
    ($_.ExecutablePath  -and ($_.ExecutablePath  -match 'Kilroy|com\.kilroy\.desktop')) -or
    ($_.CommandLine     -and ($_.CommandLine     -match 'Kilroy|com\.kilroy\.desktop'))
  }
}

function Stop-Tree {
  param([int]$ProcId, [string]$Label)
  Write-Host "  [kill] PID $ProcId  $Label" -ForegroundColor Red
  & taskkill.exe /F /T /PID $ProcId 2>$null | Out-Null
}

Write-Host ""
Write-Host "=== Kilroy force-uninstall ===" -ForegroundColor Cyan
Write-Host ""

# Phase 1 -- discover & terminate every Kilroy-related process
Write-Host "[1/4] Hunting Kilroy + WebView2 ghosts..." -ForegroundColor Cyan
$victims = Find-KilroyProcesses
if ($victims) {
  Write-Host "  found $($victims.Count) process(es):" -ForegroundColor Red
  $victims | ForEach-Object {
    $exe = if ($_.ExecutablePath) { $_.ExecutablePath } else { '(no path)' }
    Write-Host "    PID $($_.ProcessId)  $($_.Name)  $exe" -ForegroundColor DarkRed
  }
  $victims | ForEach-Object { Stop-Tree $_.ProcessId $_.Name }
  Start-Sleep -Seconds 2
} else {
  Write-Host "  no Kilroy processes running" -ForegroundColor DarkGray
}

# Sometimes WebView2 respawns once. Sweep again.
$stragglers = Find-KilroyProcesses
if ($stragglers) {
  Write-Host "  stragglers detected, second sweep:" -ForegroundColor Yellow
  $stragglers | ForEach-Object { Stop-Tree $_.ProcessId "straggler $($_.Name)" }
  Start-Sleep -Seconds 2
}

# Catch stuck NSIS uninstallers (Un_A.exe is NSIS's temp uninstaller copy)
$nsisStuck = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
  ($_.Name -eq 'Un_A.exe' -or $_.Name -eq 'uninstall.exe') -and
  $_.ExecutablePath -match 'Kilroy'
}
foreach ($p in $nsisStuck) { Stop-Tree $p.ProcessId "stuck NSIS uninstaller" }

# Phase 2 -- defer to the regular uninstaller for actual file/registry work
Write-Host "[2/4] Running regular uninstaller for files + registry..." -ForegroundColor Cyan
$splat = @{}
if ($IncludeBuild)       { $splat.IncludeBuild       = $true }
if ($IncludeProjectData) { $splat.IncludeProjectData = $true }
& "$PSScriptRoot\uninstall.ps1" @splat

# Phase 3 -- verify cleanliness
Write-Host "[3/4] Verifying clean state..." -ForegroundColor Cyan
$leftovers = @(
  "$env:LOCALAPPDATA\Programs\Kilroy",
  "$env:APPDATA\com.kilroy.desktop",
  "$env:LOCALAPPDATA\com.kilroy.desktop"
) | Where-Object { Test-Path -LiteralPath $_ }

if (-not $leftovers) {
  Write-Host "  clean -- every target removed" -ForegroundColor Green
  Write-Host ""
  Write-Host "Done. Fresh install: " -ForegroundColor Green -NoNewline
  Write-Host "npm run tauri:build" -ForegroundColor White
  Write-Host ""
  exit 0
}

Write-Host "  these paths could not be removed (still locked):" -ForegroundColor Yellow
$leftovers | ForEach-Object { Write-Host "    $_" -ForegroundColor Yellow }

# Phase 4 -- delete-on-reboot fallback
if ($NoReboot) {
  Write-Host "[4/4] Skipping delete-on-reboot (per -NoReboot)." -ForegroundColor Yellow
  Write-Host "      Reboot, then re-run this script to clean up." -ForegroundColor Yellow
  exit 1
}

Write-Host "[4/4] Scheduling locked files for delete-on-reboot..." -ForegroundColor Cyan
Add-Type -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
public static extern bool MoveFileEx(string lpExistingFileName, string lpNewFileName, int dwFlags);
'@ -Name 'Win32MoveFile' -Namespace 'Win32' -ErrorAction SilentlyContinue
$MOVEFILE_DELAY_UNTIL_REBOOT = 4

$scheduledCount = 0
foreach ($lockedRoot in $leftovers) {
  # Depth-first: children before parents so the directory can be removed.
  $items = Get-ChildItem -LiteralPath $lockedRoot -Recurse -Force -ErrorAction SilentlyContinue |
    Sort-Object { $_.FullName.Length } -Descending
  foreach ($it in $items) {
    $ok = [Win32.Win32MoveFile]::MoveFileEx($it.FullName, $null, $MOVEFILE_DELAY_UNTIL_REBOOT)
    if ($ok) { $scheduledCount++ }
    $tag = if ($ok) { 'OK' } else { 'FAIL' }
    Write-Host ("  [{0}]  {1}" -f $tag, $it.FullName) -ForegroundColor DarkYellow
  }
  $ok = [Win32.Win32MoveFile]::MoveFileEx($lockedRoot, $null, $MOVEFILE_DELAY_UNTIL_REBOOT)
  if ($ok) { $scheduledCount++ }
  $tag = if ($ok) { 'OK' } else { 'FAIL' }
  Write-Host ("  [{0}]  {1}" -f $tag, $lockedRoot) -ForegroundColor DarkYellow
}

Write-Host ""
Write-Host "$scheduledCount file(s) scheduled for delete-on-reboot." -ForegroundColor Yellow
Write-Host "Reboot Windows. The files are gone after restart. Then build fresh:" -ForegroundColor Yellow
Write-Host "  npm run tauri:build" -ForegroundColor White
Write-Host ""
