<#
.SYNOPSIS
  Completely remove Kilroy from this Windows machine.

.DESCRIPTION
  Stops any running Kilroy process, runs the NSIS uninstaller if present, then
  force-removes leftover install dir, global config dir, shortcuts, and registry
  entries. After this script returns clean, a fresh `tauri:build` + install
  produces a virgin install with no state carryover.

  Default behaviour preserves project data and build artifacts. Use the flags to
  go fully nuclear.

.PARAMETER IncludeBuild
  Also delete src-tauri/target/, src-tauri/gen/, node_modules/, and dist/ in the
  current repo. Forces a full rebuild from scratch next time.

.PARAMETER IncludeProjectData
  Also delete the .kilroy/ directory in the current working directory (project
  memory DB, project skills, staging). This is project-specific data -- only set
  if you really want to lose it.

.PARAMETER DryRun
  Print what would be removed without actually removing anything.

.EXAMPLE
  .\scripts\uninstall.ps1
  Removes installed app + global config + shortcuts + registry.

.EXAMPLE
  .\scripts\uninstall.ps1 -DryRun
  See exactly what the script would touch, without touching anything.

.EXAMPLE
  .\scripts\uninstall.ps1 -IncludeBuild -IncludeProjectData
  Nuclear: everything Kilroy ever wrote, anywhere on this machine + this repo.
#>

[CmdletBinding()]
param(
  [switch]$IncludeBuild,
  [switch]$IncludeProjectData,
  [switch]$DryRun
)

$ErrorActionPreference = 'Continue'

function Remove-Target {
  param([string]$Path, [string]$Label)
  if (-not (Test-Path -LiteralPath $Path)) {
    Write-Host "  [skip] $Label not present" -ForegroundColor DarkGray
    return
  }
  if ($DryRun) {
    Write-Host "  [DRY ] would remove $Label : $Path" -ForegroundColor Yellow
  } else {
    Write-Host "  [del ] $Label : $Path" -ForegroundColor Red
    Remove-Item -Recurse -Force -LiteralPath $Path -ErrorAction SilentlyContinue
  }
}

Write-Host ""
Write-Host "=== Kilroy uninstall ===" -ForegroundColor Cyan
if ($DryRun) { Write-Host "  (dry-run -- nothing will actually change)" -ForegroundColor Yellow }
Write-Host ""

# 1. Stop running instances -- Kilroy.exe AND its WebView2 helpers.
# Get-Process -Name 'Kilroy' misses msedgewebview2.exe children which are the
# usual culprits for held file locks. CIM lets us match by command line.
Write-Host "[1/5] Stopping Kilroy + WebView2 helper processes..." -ForegroundColor Cyan
$victims = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
  ($_.ExecutablePath -and ($_.ExecutablePath -match 'Kilroy|com\.kilroy\.desktop')) -or
  ($_.CommandLine    -and ($_.CommandLine    -match 'Kilroy|com\.kilroy\.desktop'))
}
if ($victims) {
  Write-Host "  found $($victims.Count) process(es):" -ForegroundColor Red
  $victims | ForEach-Object {
    $exe = if ($_.ExecutablePath) { $_.ExecutablePath } else { '(no path)' }
    Write-Host "    PID $($_.ProcessId)  $($_.Name)  $exe" -ForegroundColor DarkRed
  }
  if ($DryRun) {
    Write-Host "  [DRY ] would kill all of the above with taskkill /F /T" -ForegroundColor Yellow
  } else {
    $victims | ForEach-Object {
      & taskkill.exe /F /T /PID $_.ProcessId 2>$null | Out-Null
    }
    Start-Sleep -Seconds 2
    # WebView2 sometimes respawns once. Sweep again.
    $stragglers = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
      ($_.ExecutablePath -and ($_.ExecutablePath -match 'Kilroy|com\.kilroy\.desktop')) -or
      ($_.CommandLine    -and ($_.CommandLine    -match 'Kilroy|com\.kilroy\.desktop'))
    }
    if ($stragglers) {
      Write-Host "  $($stragglers.Count) straggler(s) respawned, killing again..." -ForegroundColor Yellow
      $stragglers | ForEach-Object { & taskkill.exe /F /T /PID $_.ProcessId 2>$null | Out-Null }
      Start-Sleep -Seconds 1
    }
    Write-Host "  done" -ForegroundColor Red
  }
} else {
  Write-Host "  no Kilroy processes running" -ForegroundColor DarkGray
}

# 2. Run NSIS uninstaller silently if it's there. This handles Add/Remove
# Programs registration cleanup. Force-removal in step 3 mops up anything left.
Write-Host "[2/5] Running NSIS uninstaller (if installed)..." -ForegroundColor Cyan
$uninst = "$env:LOCALAPPDATA\Programs\Kilroy\uninstall.exe"
if (Test-Path -LiteralPath $uninst) {
  if ($DryRun) {
    Write-Host "  [DRY ] would run: $uninst /S" -ForegroundColor Yellow
  } else {
    Write-Host "  running: $uninst /S" -ForegroundColor Red
    Start-Process -FilePath $uninst -ArgumentList '/S' -Wait
    Start-Sleep -Seconds 2
  }
} else {
  Write-Host "  not installed via NSIS -- skipping" -ForegroundColor DarkGray
}

# 3. Force-remove install dir + global config dir
Write-Host "[3/5] Removing app + global config..." -ForegroundColor Cyan
Remove-Target "$env:LOCALAPPDATA\Programs\Kilroy"          "install dir (Kilroy.exe + uninstaller)"
Remove-Target "$env:APPDATA\com.kilroy.desktop"            "global config (settings.json, global skills, window state, WebView2 cache)"
Remove-Target "$env:LOCALAPPDATA\com.kilroy.desktop"       "local cache (some Tauri builds put state here)"

# 4. Remove shortcuts (currentUser + machine-wide, in case both were created)
Write-Host "[4/5] Removing shortcuts..." -ForegroundColor Cyan
Remove-Target "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Kilroy.lnk"     "Start menu shortcut (user)"
Remove-Target "$env:USERPROFILE\Desktop\Kilroy.lnk"                                "Desktop shortcut (user)"
Remove-Target "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Kilroy.lnk" "Start menu shortcut (all users)"
Remove-Target "$env:PUBLIC\Desktop\Kilroy.lnk"                                     "Desktop shortcut (all users)"

# 5. Clean orphaned registry entries
Write-Host "[5/5] Cleaning registry..." -ForegroundColor Cyan
$regBases = @(
  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
  'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
)
$hits = 0
foreach ($base in $regBases) {
  $entries = Get-ChildItem -LiteralPath $base -ErrorAction SilentlyContinue | Where-Object {
    $p = Get-ItemProperty -LiteralPath $_.PSPath -ErrorAction SilentlyContinue
    $p.DisplayName -eq 'Kilroy' -or $p.Publisher -eq 'Kilroy'
  }
  foreach ($e in $entries) {
    $hits++
    if ($DryRun) {
      Write-Host "  [DRY ] would delete: $($e.PSPath)" -ForegroundColor Yellow
    } else {
      Write-Host "  [del ] $($e.PSPath)" -ForegroundColor Red
      Remove-Item -Recurse -Force -LiteralPath $e.PSPath -ErrorAction SilentlyContinue
    }
  }
}
if ($hits -eq 0) {
  Write-Host "  no Kilroy registry entries found" -ForegroundColor DarkGray
}

# Optional: build artifacts in the current repo
if ($IncludeBuild) {
  Write-Host ""
  Write-Host "[+] Removing build artifacts in current repo..." -ForegroundColor Magenta
  Remove-Target "src-tauri\target"   "Rust build cache (~5+ GB)"
  Remove-Target "src-tauri\gen"      "Tauri generated capabilities + Cargo lockfile shim"
  Remove-Target "node_modules"      "npm modules"
  Remove-Target "dist"               "Vite build output"
  Remove-Target ".vite"              "Vite cache"
}

# Optional: project-local data
if ($IncludeProjectData) {
  Write-Host ""
  Write-Host "[+] Removing project-local .kilroy/ data in current dir..." -ForegroundColor Magenta
  Remove-Target ".kilroy"           "project memory DB, project skills, staging, sandboxes"
}

Write-Host ""
if ($DryRun) {
  Write-Host "Dry-run complete. Re-run without -DryRun to actually remove." -ForegroundColor Yellow
} else {
  Write-Host "Done. Kilroy is gone." -ForegroundColor Green
  Write-Host "Fresh install: " -NoNewline -ForegroundColor Green
  Write-Host "npm run tauri:build" -ForegroundColor White -NoNewline
  Write-Host " -> run the new .exe in src-tauri\target\release\bundle\nsis\" -ForegroundColor Green
}
Write-Host ""
Write-Host "Note: this script does not touch Ollama or its models." -ForegroundColor DarkGray
Write-Host "      To remove Ollama too: " -ForegroundColor DarkGray -NoNewline
Write-Host "winget uninstall Ollama.Ollama" -ForegroundColor Gray
Write-Host ""
