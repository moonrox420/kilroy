<#
.SYNOPSIS
  Kilroy build / runtime doctor. Checks every prerequisite for both
  dev work and producing a consumer-ready installer.

.DESCRIPTION
  Validates the local toolchain and reports a punch-list of anything
  missing or out-of-date. Run before tauri:build to catch environment
  drift instead of hitting cryptic build errors mid-compile.

  Exits 0 if everything passes, 1 if any check fails. Always prints
  the full report.

.PARAMETER Strict
  Treat warnings as errors (e.g. outdated minor versions).

.EXAMPLE
  npm run doctor
  Standard report.

.EXAMPLE
  .\scripts\doctor.ps1 -Strict
  Fail on any non-green check.
#>

[CmdletBinding()]
param([switch]$Strict)

$ErrorActionPreference = 'Continue'

$results = @()
function Check {
  param(
    [string]$Name,
    [scriptblock]$Probe,
    [string]$FixHint = "",
    [switch]$AsWarning
  )
  $row = [pscustomobject]@{
    Name    = $Name
    Status  = "?"
    Detail  = ""
    FixHint = $FixHint
  }
  try {
    $detail = & $Probe
    if ($null -ne $detail -and $detail -ne $false) {
      $row.Status = "OK"
      $row.Detail = "$detail"
    } else {
      $row.Status = if ($AsWarning) { "WARN" } else { "FAIL" }
    }
  } catch {
    $row.Status = if ($AsWarning) { "WARN" } else { "FAIL" }
    $row.Detail = $_.Exception.Message
  }
  $script:results += $row
  return $row
}

Write-Host ""
Write-Host "=== Kilroy doctor ===" -ForegroundColor Cyan
Write-Host ""

# Node
Check -Name "Node.js" -Probe {
  $v = (node --version 2>$null)
  if ($v -match '^v(\d+)') {
    $major = [int]$Matches[1]
    if ($major -lt 18) { throw "Node $v is too old. Need >= 18." }
    return $v
  } else { throw "node not on PATH" }
} -FixHint "winget install OpenJS.NodeJS.LTS"

# npm
Check -Name "npm" -Probe { (npm --version 2>$null) } `
  -FixHint "Bundled with Node -- reinstall Node if missing."

# Rust toolchain -- must be >= 1.95.0. libsqlite3-sys 0.38+ (a transitive
# dep via rusqlite) uses the `cfg_select!` macro in its build script, and
# that only stabilized in Rust 1.95.0. Older stable fails with E0658.
Check -Name "Rust (rustc >= 1.95)" -Probe {
  $v = (rustc --version 2>$null)
  if (-not $v) { throw "rustc not on PATH" }
  if ($v -match '(\d+)\.(\d+)\.(\d+)') {
    $ver = [version]("{0}.{1}.{2}" -f $Matches[1], $Matches[2], $Matches[3])
    if ($ver -lt [version]"1.95.0") {
      throw "$ver is too old (need >= 1.95.0 for cfg_select!). Run: rustup update stable"
    }
  }
  return $v
} -FixHint "rustup update stable   (Kilroy needs rustc >= 1.95.0; cfg_select! stabilized there)"

Check -Name "Cargo" -Probe { (cargo --version 2>$null) } `
  -FixHint "Comes with rustup."

# Tauri CLI (project-local first, global fallback)
Check -Name "Tauri CLI" -Probe {
  $local = (npx --no-install @tauri-apps/cli --version 2>$null)
  if ($local) { return "$local (project)" }
  $global = (tauri --version 2>$null)
  if ($global) { return "$global (global)" }
  throw "@tauri-apps/cli not in node_modules -- run npm install"
} -FixHint "npm install"

# Visual Studio Build Tools (Rust on Windows needs MSVC linker)
Check -Name "MSVC build tools" -Probe {
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  if (-not (Test-Path $vswhere)) { throw "vswhere.exe not found" }
  $info = & $vswhere -latest -products * -requires Microsoft.VisualCpp.Tools.x86.x64 -property installationVersion 2>$null
  if ($info) { return "VS Build Tools $info" } else { throw "MSVC C++ tools not installed" }
} -FixHint "winget install Microsoft.VisualStudio.2022.BuildTools (then add 'Desktop development with C++' workload)"

# WebView2 runtime
Check -Name "WebView2 Runtime" -Probe {
  $hk = "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
  if (Test-Path $hk) {
    $v = (Get-ItemProperty $hk -Name pv -ErrorAction SilentlyContinue).pv
    if ($v) { return "v$v" }
  }
  # Win11 ships with WebView2 -- check system path too.
  $sys = "${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application"
  if (Test-Path $sys) { return "system" }
  throw "Not installed"
} -FixHint "Pre-installed on Win11. On Win10: download from https://go.microsoft.com/fwlink/p/?LinkId=2124703"

# Ollama (system-wide)
Check -Name "Ollama (system)" -Probe {
  $v = (ollama --version 2>$null)
  if ($v) { return $v } else { throw "Not on PATH" }
} -FixHint "winget install Ollama.Ollama  -- or it'll be installed bundled when Kilroy runs."

# Ollama daemon reachable
Check -Name "Ollama daemon" -Probe {
  try {
    $r = Invoke-WebRequest -Uri http://localhost:11434/api/tags -TimeoutSec 2 -UseBasicParsing
    if ($r.StatusCode -eq 200) { return "reachable" } else { throw "HTTP $($r.StatusCode)" }
  } catch {
    throw "not reachable"
  }
} -FixHint "Start: ollama serve  -- or wait for the bundled daemon if you're inside Kilroy"

# Configured chat model -- read from settings.json, NOT hardcoded. Kilroy
# is model-agnostic: any Ollama-compatible tag works (qwen2.5-coder, llama3,
# deepseek-coder, mistral, mixtral, phi, codestral, etc.). The doctor only
# verifies whatever the user has selected in settings.
Check -Name "Configured chat model" -Probe {
  # Locate settings.json under Tauri's app config dir.
  $cfg  = Join-Path $env:APPDATA "com.kilroy.desktop\settings.json"
  $want = $null
  if (Test-Path $cfg) {
    try {
      $settings = Get-Content $cfg -Raw | ConvertFrom-Json
      if ($settings.chat_model) { $want = [string]$settings.chat_model }
    } catch { }
  }
  # Env-var override mirrors what AppState reads at startup.
  if ($env:KILROY_CHAT_MODEL) { $want = $env:KILROY_CHAT_MODEL }
  if (-not $want) {
    # Settings file not written yet (first run hasn't happened). Don't
    # fail -- just report unknown so doctor stays green on a fresh box.
    return "settings.json not present yet (will be created on first run)"
  }
  $r = Invoke-WebRequest -Uri http://localhost:11434/api/tags -TimeoutSec 2 -UseBasicParsing -ErrorAction SilentlyContinue
  if (-not $r) { throw "Ollama unreachable" }
  $body = $r.Content | ConvertFrom-Json
  # Match by exact tag first, then by the bare model name (handles "qwen2.5-coder:14b-instruct-q8_0" vs "qwen2.5-coder:latest").
  $bareWant = ($want -split ':')[0]
  $found = $body.models | Where-Object { $_.name -eq $want -or $_.name -like "$bareWant*" } | Select-Object -First 1
  if ($found) { return "$($found.name)  (want=$want)" } else { throw "configured model `"$want`" not pulled" }
} -FixHint "ollama pull qwen2.5-coder:7b-instruct-q8_0  (default suggestion: qwen2.5-coder:7b-instruct-q8_0, ~8.1 GB)"

# Configured embedding model -- same pattern, model-agnostic.
Check -Name "Configured embedding model" -Probe {
  $cfg  = Join-Path $env:APPDATA "com.kilroy.desktop\settings.json"
  $want = $null
  if (Test-Path $cfg) {
    try {
      $settings = Get-Content $cfg -Raw | ConvertFrom-Json
      if ($settings.embedding_model) { $want = [string]$settings.embedding_model }
    } catch { }
  }
  if ($env:KILROY_EMBEDDING_MODEL) { $want = $env:KILROY_EMBEDDING_MODEL }
  if (-not $want) { return "settings.json not present yet" }
  $r = Invoke-WebRequest -Uri http://localhost:11434/api/tags -TimeoutSec 2 -UseBasicParsing -ErrorAction SilentlyContinue
  if (-not $r) { throw "Ollama unreachable" }
  $body = $r.Content | ConvertFrom-Json
  $bareWant = ($want -split ':')[0]
  $found = $body.models | Where-Object { $_.name -eq $want -or $_.name -like "$bareWant*" } | Select-Object -First 1
  if ($found) { return "$($found.name)  (want=$want)" } else { throw "configured embedding model `"$want`" not pulled" }
} -FixHint "ollama pull nomic-embed-text  (default suggestion: nomic-embed-text, ~270 MB)"

# Windows Sandbox feature -- WARN-level, never blocks the build. Windows 11
# Home does not ship the Containers-DisposableClientVM feature at all, and
# Kilroy degrades gracefully at runtime (host / docker isolation remain
# selectable in Settings). Pass -Strict to treat this as fatal.
Check -Name "Windows Sandbox" -AsWarning -Probe {
  $f = Get-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -ErrorAction SilentlyContinue
  if ($f -and $f.State -eq "Enabled") { return "enabled" } else { throw "not enabled (unavailable on Windows Home; optional elsewhere)" }
} -FixHint "Pro/Enterprise only: Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All  (needs elevation + reboot)"

# ─── SmartCoder (Python Code Agent) ──────────────────────────────────────

# Python 3.10+ for SmartCoder
Check -Name "Python (>= 3.10)" -Probe {
  $v = (python --version 2>$null)
  if (-not $v) { throw "python not on PATH" }
  if ($v -match '(\d+)\.(\d+)') {
    $major = [int]$Matches[1]
    $minor = [int]$Matches[2]
    if ($major -lt 3 -or ($major -eq 3 -and $minor -lt 10)) {
      throw "$v is too old. Need >= 3.10."
    }
  }
  return $v
} -FixHint "winget install Python.Python.3.12  -- or install from python.org"

# SmartCoder venv (project .venv)
Check -Name "kilroy venv" -Probe {
  $venv = Join-Path $PWD "kilroy\.venv"
  if (Test-Path (Join-Path $venv "pyvenv.cfg")) {
    $python = Join-Path $venv "Scripts\python.exe"
    if (Test-Path $python) {
      $v = (& $python --version 2>$null)
      return "$v  ($venv)"
    }
    return "pyvenv.cfg found but python.exe missing"
  }
  throw "kilroy/.venv not found"
} -FixHint "cd kilroy && python -m venv .venv && .venv\Scripts\activate"

# pip check (no broken deps)
Check -Name "kilroy pip check" -Probe {
  $venv = Join-Path $PWD "kilroy\.venv"
  $pip = Join-Path $venv "Scripts\pip.exe"
  if (-not (Test-Path $pip)) { throw "pip not found in .venv" }
  $out = & $pip check 2>&1
  if ($LASTEXITCODE -eq 0) { return "all dependencies satisfied" }
  throw $out
} -FixHint "cd kilroy && .venv\Scripts\activate"

# FAISS index (optional — retrieval works without it, just slower)
Check -Name "FAISS index (optional)" -AsWarning -Probe {
  $index = Join-Path $PWD "smartcoder\vector_store\index.faiss"
  $pkl   = Join-Path $PWD "smartcoder\vector_store\index.pkl"
  if ((Test-Path $index) -and (Test-Path $pkl)) {
    $size = (Get-Item $index).Length
    return "$([math]::Round($size/1KB,1)) KB"
  }
  throw "vector_store/index.faiss or index.pkl missing (run: smartcoder build-index)"
} -FixHint "cd kilroy && .venv\Scripts\python -m kilroy_retrieval"

# Bundled Ollama (build-time prereq)
Check -Name "Bundled Ollama (build)" -Probe {
  $p = "src-tauri\resources\ollama\ollama.exe"
  if (Test-Path $p) {
    $size = (Get-Item $p).Length
    return "$([math]::Round($size/1MB,1)) MB"
  } else { throw "not fetched" }
} -FixHint "npm run fetch:ollama"

# Print report
Write-Host ""
$ok = ($results | Where-Object { $_.Status -eq "OK" }).Count
$warn = ($results | Where-Object { $_.Status -eq "WARN" }).Count
$fail = ($results | Where-Object { $_.Status -eq "FAIL" }).Count
foreach ($r in $results) {
  $color = switch ($r.Status) {
    "OK"   { "Green" }
    "WARN" { "Yellow" }
    default { "Red" }
  }
  $line = "  [{0}] {1,-28} {2}" -f $r.Status, $r.Name, $r.Detail
  Write-Host $line -ForegroundColor $color
  if ($r.Status -ne "OK" -and $r.FixHint) {
    Write-Host ("       " + $r.FixHint) -ForegroundColor DarkGray
  }
}
Write-Host ""
$summaryColor = if ($fail -gt 0) { "Red" } elseif ($warn -gt 0) { "Yellow" } else { "Green" }
Write-Host "$ok passed, $warn warned, $fail failed." -ForegroundColor $summaryColor
Write-Host ""

if ($fail -gt 0 -or ($Strict -and $ok -ne $results.Count)) { exit 1 } else { exit 0 }