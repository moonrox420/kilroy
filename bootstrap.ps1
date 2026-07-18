<#
.SYNOPSIS
    Bootstrap Kilroy on a fresh Windows 11 machine.

.DESCRIPTION
    One command from a clean PowerShell to a running Kilroy. Verifies
    prerequisites (Rust, Node, Python, MSVC build tools, Ollama, WebView2),
    optionally enables Windows Sandbox, pulls the default chat +
    embedding models, sets up the SmartCoder Python environment, runs
    `npm install` + `cargo fetch`, then launches dev mode. Idempotent —
    re-run any time.

    Run it from the extracted project root:

        cd C:\dev\kilroy
        .\bootstrap.ps1

.PARAMETER ProjectRoot
    Path to the extracted Kilroy project. Defaults to the script's directory.

.PARAMETER SkipSandbox
    Skip Windows Sandbox feature checks.

.PARAMETER SkipModels
    Skip pulling Ollama models.

.PARAMETER SkipSmartcoder
    Skip SmartCoder Python environment setup (uv venv + uv pip install -r requirements.txt).
.PARAMETER SkipIndex
    Skip the `smartcoder build-index` step (only meaningful when
    SmartCoder setup is not skipped).

.PARAMETER NoRun
    Set everything up but don't launch dev mode at the end.

.PARAMETER Build
    After setup, run `npm run build:release` to produce the consumer NSIS
    installer (runs doctor, fetches the bundled Ollama, then builds; emits
    signed auto-update artifacts when a signing key is present) instead of
    launching dev mode.

.PARAMETER ChatModel
    Ollama tag to pull as the chat / planner model. Default: qwen2.5-coder:14b-instruct-q8_0

.PARAMETER EmbedModel
    Ollama tag to pull as the embedding model. Default: nomic-embed-text

.EXAMPLE
    .\bootstrap.ps1
        # full clean-machine bootstrap, ends with `npm run tauri:dev` running

.EXAMPLE
    .\bootstrap.ps1 -SkipModels -NoRun
        # just verify the toolchain + install npm deps; don't pull models or run

.EXAMPLE
    .\bootstrap.ps1 -Build
        # bootstrap then produce the release NSIS installer via build:release

.EXAMPLE
    .\bootstrap.ps1 -SkipSmartcoder -SkipIndex
        # skip all Python / SmartCoder setup — only Rust + Node + Ollama + deps
#>

#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ProjectRoot = $PSScriptRoot,
    [switch]$SkipSandbox,
    [switch]$SkipModels,
    [switch]$SkipSmartcoder,
    [switch]$SkipIndex,
    [switch]$NoRun,
    [switch]$Build,
    [string]$ChatModel = "qwen2.5-coder:7b-instruct-q8_0",
    [string]$EmbedModel = "nomic-embed-text"
)

$ErrorActionPreference = "Stop"
$started = Get-Date

# Make Windows PowerShell 5.1 and the Windows Console Host render UTF-8
# output correctly. PowerShell 7+ already does.
try { [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new() } catch { }

# --- theme + helpers -------------------------------------------------------

$script:Amber = "Yellow"
$script:Ok = "Green"
$script:ErrColor = "Red"
$script:Muted = "DarkGray"

function Write-Banner {
    Write-Host ""
    Write-Host "  Kilroy bootstrap" -ForegroundColor $script:Amber
    Write-Host "  -----------------------------------------------" -ForegroundColor $script:Muted
    Write-Host "  Local AI Agentic Engineering Runtime" -ForegroundColor $script:Muted
    Write-Host ""
}

function Write-Phase([string]$n, [string]$total, [string]$title) {
    Write-Host ""
    Write-Host "[$n/$total] $title" -ForegroundColor Cyan
    Write-Host "        " -NoNewline
    Write-Host ("-" * ($title.Length + 10)) -ForegroundColor $script:Muted
}

function Write-Ok([string]$msg)   { Write-Host "  OK $msg" -ForegroundColor $script:Ok }
function Write-Warn([string]$msg) { Write-Host "  ! $msg" -ForegroundColor $script:Amber }
function Write-Fail([string]$msg) { Write-Host "  X $msg" -ForegroundColor $script:ErrColor }
function Write-Info([string]$msg) { Write-Host "  - $msg" -ForegroundColor $script:Muted }

function Test-Command([string]$cmd) {
    return $null -ne (Get-Command $cmd -ErrorAction SilentlyContinue)
}

function Test-Admin {
    $id = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    $p = New-Object System.Security.Principal.WindowsPrincipal($id)
    return $p.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Sync-PathEnv {
    # Pull the latest Machine + User PATH so commands installed in this
    # session become available without a shell restart. `Sync` is on the
    # PowerShell approved-verbs list; the older `Refresh` is not.
    $env:Path = (
        [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
        [System.Environment]::GetEnvironmentVariable("Path", "User")
    )
}

function Install-Winget([string]$id, [string]$friendly = $id, [string[]]$extraArgs = @()) {
    Write-Info "installing $friendly via winget..."
    # NOTE: we use `$cmdArgs` (not `$args`) to avoid clobbering PowerShell's
    # auto-variable for unbound parameters.
    $cmdArgs = @(
        "install", "--id", $id, "-e",
        "--accept-source-agreements", "--accept-package-agreements",
        "--silent"
    ) + $extraArgs
    & winget @cmdArgs 2>&1 | ForEach-Object { Write-Host "    $_" -ForegroundColor $script:Muted }
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189) {
        # -1978335189 = "package already installed", treat as success
        Write-Warn "winget exited $LASTEXITCODE for $id (continuing)"
    }
    Sync-PathEnv
}

# --- go --------------------------------------------------------------------

Write-Banner

if (-not (Test-Path $ProjectRoot)) {
    Write-Fail "ProjectRoot does not exist: $ProjectRoot"
    exit 1
}
$pkg = Join-Path $ProjectRoot "package.json"
$cargo = Join-Path $ProjectRoot "src-tauri\Cargo.toml"
$cargoRoot = Join-Path $ProjectRoot "Cargo.toml"
$cargoPath = if (Test-Path $cargo) { $cargo } elseif (Test-Path $cargoRoot) { $cargoRoot } else { $null }
if (-not (Test-Path $pkg) -or -not $cargoPath) {
    Write-Fail "Not a Kilroy project root (missing package.json or Cargo.toml)."
    Write-Fail "Pass -ProjectRoot or cd into the extracted folder before running."
    exit 1
}
Set-Location $ProjectRoot
Write-Ok "project root: $ProjectRoot"

# --- 1/7: winget itself ----------------------------------------------------

Write-Phase 1 7 "winget"
if (-not (Test-Command winget)) {
    Write-Fail "winget is missing -- install 'App Installer' from the Microsoft Store, then re-run."
    Start-Process "ms-windows-store://pdp/?ProductId=9NBLGGH4NNS1"
    exit 1
}
Write-Ok "winget present"

# --- 2/7: toolchain (Rust, Node, Python, MSVC, WebView2) -------------------

Write-Phase 2 7 "toolchain (Rust, Node, Python, MSVC, WebView2)"

# Rust.
#
# Kilroy needs rustc >= 1.95.0. A transitive dependency -- libsqlite3-sys
# 0.38+ (pulled in by rusqlite) -- uses the `cfg_select!` macro in its
# build script, and `cfg_select!` only stabilized in Rust 1.95.0. On an
# older stable toolchain the build fails with:
#     error[E0658]: use of unstable library feature `cfg_select`
# We fix this the clean way -- keep the crate dependency unpinned and make
# sure the toolchain is new enough -- by auto-running `rustup update`.
$MIN_RUST = [version]"1.95.0"

function Get-RustcVersion {
    if (-not (Test-Command rustc)) { return $null }
    $raw = (& rustc --version)
    if ($raw -match '(\d+)\.(\d+)\.(\d+)') {
        return [version]("{0}.{1}.{2}" -f $Matches[1], $Matches[2], $Matches[3])
    }
    return $null
}

if (-not (Test-Command rustc)) {
    Install-Winget "Rustlang.Rustup" "Rust (rustup)"
    Sync-PathEnv
    if (Test-Command rustup) {
        & rustup default stable | Out-Null
        & rustup target add x86_64-pc-windows-msvc | Out-Null
    }
}

if (Test-Command rustc) {
    $rustVer = Get-RustcVersion
    if ($rustVer -and ($rustVer -lt $MIN_RUST)) {
        Write-Warn ("rustc $rustVer is older than $MIN_RUST -- updating toolchain " +
                    "(libsqlite3-sys needs cfg_select!, stabilized in Rust 1.95.0)...")
        if (Test-Command rustup) {
            & rustup update stable | Out-Null
            & rustup default stable | Out-Null
            Sync-PathEnv
            $rustVer = Get-RustcVersion
        } else {
            Write-Warn "rustup not found -- run 'rustup update stable' manually, then re-run."
        }
    }
    if ($rustVer -and ($rustVer -ge $MIN_RUST)) {
        Write-Ok ("rustc: " + (& rustc --version))
    } else {
        Write-Warn ("rustc " + (& rustc --version) +
                    " -- Kilroy needs >= 1.95.0 to compile. Run 'rustup update stable' and re-run bootstrap.")
    }
} else {
    Write-Warn "rustc not found after install -- open a new PowerShell window and re-run."
}

# Node LTS
if (-not (Test-Command node)) {
    Install-Winget "OpenJS.NodeJS.LTS" "Node.js LTS"
}
if (Test-Command node) {
    Write-Ok ("node:  " + (& node --version))
    Write-Ok ("npm:   " + (& npm --version))
} else {
    Write-Warn "node not found after install -- open a new PowerShell window and re-run."
}

# Python 3.10+ (required by SmartCoder)
$MIN_PYTHON = [version]"3.10"
function Get-PythonVersion {
    # Try `python --version` first; fall back to `python3`.
    $raw = $null
    try {
        $raw = & python --version 2>&1
    } catch { }
    if (-not $raw) {
        try { $raw = & python3 --version 2>&1 } catch { }
    }
    if ($raw -match '(\d+)\.(\d+)\.(\d+)') {
        return [version]("{0}.{1}.{2}" -f $Matches[1], $Matches[2], $Matches[3])
    }
    return $null
}
$pyVer = Get-PythonVersion
if (-not $pyVer) {
    Write-Info "Python not found -- installing Python 3.12 via winget..."
    Install-Winget "Python.Python.3.12" "Python 3.12"
    Sync-PathEnv
    $pyVer = Get-PythonVersion
}
if ($pyVer -and $pyVer -ge $MIN_PYTHON) {
    Write-Ok ("python: " + (& python --version 2>&1).Trim())
} else {
    Write-Warn ("Python $pyVer is too old. SmartCoder needs >= 3.10. " +
                "Install Python 3.12+ from https://www.python.org/downloads/ and re-run.")
}

# MSVC build tools -- required for the Rust linker on Windows.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$hasMsvc = $false
if (Test-Path $vswhere) {
    $vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    if ($vsPath) {
        $hasMsvc = $true
        Write-Ok "MSVC C++ Build Tools detected at $vsPath"
    }
}
if (-not $hasMsvc) {
    Write-Info "MSVC C++ Build Tools missing -- pulling them now (~3 GB)..."
    Install-Winget "Microsoft.VisualStudio.2022.BuildTools" "VS 2022 Build Tools" `
        @("--override", '"--passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"')
}

# WebView2 runtime ships with Windows 11 -- guard anyway.
$wv2 = Get-ItemProperty -Path "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" -ErrorAction SilentlyContinue
if (-not $wv2) {
    Install-Winget "Microsoft.EdgeWebView2Runtime" "Edge WebView2 Runtime"
} else {
    Write-Ok "WebView2 runtime present"
}

# --- 3/7: SmartCoder Python environment ------------------------------------

Write-Phase 3 7 "SmartCoder (Python RAG coding agent)"

if ($SkipSmartcoder) {
    Write-Info "skipped via -SkipSmartcoder"
} else {
    $smartDir = Join-Path $ProjectRoot "smartcoder"
    if (-not (Test-Path $smartDir)) {
        Write-Warn "smartcoder/ directory not found -- run from the project root that has the smartcoder/ subfolder."
    } else {
        $venvDir = Join-Path $smartDir ".venv"
        $reqsPath = Join-Path $ProjectRoot "requirements.txt"

        if (-not (Test-Path (Join-Path $venvDir "Scripts\python.exe"))) {
            Write-Info "creating Python virtual environment at $venvDir..."
            & uv venv $venvDir 2>&1 | ForEach-Object { Write-Host "    $_" -ForegroundColor $script:Muted }
            if ($LASTEXITCODE -ne 0) {
                Write-Warn "uv venv creation failed -- ensure uv is installed and available on PATH."
            } else {
                Write-Ok "venv created"
            }
        } else {
            Write-Ok "venv already exists"
        }

        $pythonExe = Join-Path $venvDir "Scripts\python.exe"
        if (Test-Path $pythonExe) {
            Write-Info "installing SmartCoder deps from requirements.txt..."
            & uv pip install -r $reqsPath 2>&1 | ForEach-Object { Write-Host "    $_" -ForegroundColor $script:Muted }
            if ($LASTEXITCODE -eq 0) {
                Write-Ok "SmartCoder deps installed in venv"
            } else {
                Write-Warn "uv pip install exited $LASTEXITCODE -- check that uv is available and all deps resolve."
            }

            # Optional: build the FAISS index (can take a while).
            if (-not $SkipIndex) {
                $smartcoderCli = Join-Path $venvDir "Scripts\smartcoder.exe"
                if (Test-Path $smartcoderCli) {
                    Write-Info "building FAISS index (first build downloads datasets: ~500 MB, then chunk + embed)..."
                    Write-Info "this can take several minutes. Pass -SkipIndex to defer."
                    & $smartcoderCli build-index 2>&1 | ForEach-Object { Write-Host "    $_" -ForegroundColor $script:Muted }
                    if ($LASTEXITCODE -eq 0) {
                        Write-Ok "FAISS index built"
                    } else {
                        Write-Warn "smartcoder build-index exited $LASTEXITCODE (can be re-run later with: smartcoder build-index)"
                    }
                } else {
                    Write-Warn "smartcoder CLI not found in venv after install -- build-index step skipped."
                    Write-Info "when the install works, run: .\kilroy\.venv\Scripts\smartcoder build-index"
                }
            } else {
                Write-Info "FAISS index build skipped via -SkipIndex"
            }
        } else {
            Write-Warn "python not found in venv at $pythonExe -- SmartCoder setup incomplete."
        }
    }
}

# --- 4/7: Ollama -----------------------------------------------------------

Write-Phase 4 7 "Ollama service + models"
if (-not (Test-Command ollama)) {
    Install-Winget "Ollama.Ollama" "Ollama"
}

# Make sure the daemon is reachable; start it if not.
$reachable = $false
try {
    Invoke-RestMethod -Uri "http://localhost:11434/api/tags" -TimeoutSec 2 -ErrorAction Stop | Out-Null
    $reachable = $true
} catch { }
if (-not $reachable) {
    Write-Info "starting `ollama serve` in the background..."
    Start-Process "ollama.exe" -ArgumentList "serve" -WindowStyle Hidden
    Start-Sleep 3
    try {
        Invoke-RestMethod -Uri "http://localhost:11434/api/tags" -TimeoutSec 5 -ErrorAction Stop | Out-Null
        $reachable = $true
    } catch { }
}
if ($reachable) {
    Write-Ok "Ollama reachable on http://localhost:11434"
} else {
    Write-Warn "Ollama unreachable -- Kilroy will still launch but model calls will fail until you start it."
}

if (-not $SkipModels -and $reachable) {
    $installed = (& ollama list 2>$null) -join "`n"
    foreach ($model in @($EmbedModel, $ChatModel)) {
        $modelBase = ($model -split ":")[0]
        if ($installed -match [regex]::Escape($modelBase)) {
            Write-Ok "$model already pulled"
        } else {
            Write-Info "pulling $model (this can take a while)..."
            & ollama pull $model
            if ($LASTEXITCODE -eq 0) {
                Write-Ok "$model pulled"
            } else {
                Write-Warn "ollama pull $model exited $LASTEXITCODE"
            }
        }
    }
}

# --- 5/7: Windows Sandbox feature ------------------------------------------

Write-Phase 5 7 "Windows Sandbox feature"
if ($SkipSandbox) {
    Write-Info "skipped via -SkipSandbox"
    } else {
    try {
    $feat = Get-WindowsOptionalFeature -Online -FeatureName "Containers-DisposableClientVM" -ErrorAction Stop
    } catch {
    $feat = $null
    }
    if (-not $feat) {
        Write-Warn "could not query the feature (running on a Home edition or restricted Windows?)."
    } elseif ($feat.State -eq "Enabled") {
        Write-Ok "Windows Sandbox feature enabled"
    } else {
        if (Test-Admin) {
            Write-Info "enabling Windows Sandbox (Containers-DisposableClientVM)..."
            Enable-WindowsOptionalFeature -Online -FeatureName "Containers-DisposableClientVM" -All -NoRestart | Out-Null
            Write-Warn "feature enabled -- REBOOT required before WindowsSandbox.exe is on PATH."
        } else {
            Write-Warn "Windows Sandbox is disabled. Re-run this script from an elevated PowerShell, or run manually:"
            Write-Warn "    Enable-WindowsOptionalFeature -Online -FeatureName `"Containers-DisposableClientVM`" -All"
            Write-Warn "Until then, Kilroy works but Windows-Sandbox shell actions fail; flip Settings -> Sandbox -> Host (or Docker, if installed)."
        }
    }
}

# --- 6/7: frontend + Rust deps ---------------------------------------------

Write-Phase 6 7 "frontend + Rust deps"

$nodeModules = Join-Path $ProjectRoot "node_modules"
$packageLock = Join-Path $ProjectRoot "package-lock.json"
if ((Test-Path $nodeModules) -and (Test-Path $packageLock)) {
    Write-Ok "node_modules already populated -- running `npm ci` to honour the lockfile"
    & npm ci 2>&1 | ForEach-Object { Write-Host "    $_" -ForegroundColor $script:Muted }
} else {
    Write-Info "running `npm install` (first run, ~600 MB)..."
    & npm install 2>&1 | ForEach-Object { Write-Host "    $_" -ForegroundColor $script:Muted }
}
if ($LASTEXITCODE -ne 0) {
    Write-Fail "npm install failed -- see output above."
    exit 1
}
Write-Ok "npm dependencies installed"

# Prefetch Rust crates so the first `tauri dev` is closer to instant.
Write-Info "pre-fetching Rust crates (cargo fetch)..."
Push-Location (Join-Path $ProjectRoot "src-tauri")
try {
    & cargo fetch 2>&1 | ForEach-Object { Write-Host "    $_" -ForegroundColor $script:Muted }
    if ($LASTEXITCODE -eq 0) { Write-Ok "cargo fetch complete" }
    else { Write-Warn "cargo fetch exited $LASTEXITCODE (will retry on first build)" }
} finally {
    Pop-Location
}

# --- 7/7: run --------------------------------------------------------------

Write-Phase 7 7 "launch"
$elapsed = (Get-Date) - $started
Write-Ok ("setup took " + [int]$elapsed.TotalSeconds + "s")

if ($NoRun) {
    Write-Host ""
    Write-Host "  Bootstrap complete." -ForegroundColor $script:Ok
    Write-Host "  To run dev mode:    " -NoNewline; Write-Host "npm run tauri:dev" -ForegroundColor $script:Amber
    Write-Host "  To build installer: " -NoNewline; Write-Host "npm run build:release" -ForegroundColor $script:Amber
    Write-Host "  To bump deps:       " -NoNewline
    Write-Host "npm run bump" -ForegroundColor $script:Amber
    Write-Host "                      " -NoNewline
    Write-Host "(needs cargo-outdated: cargo install cargo-outdated)" -ForegroundColor $script:Muted
    Write-Host "  SmartCoder CLI:     " -NoNewline
    Write-Host ".\smartcoder\.venv\Scripts\smartcoder --help" -ForegroundColor $script:Amber
    Write-Host ""
    Write-Host "  ⚠ SECURITY: .env contains your Tauri signing key." -ForegroundColor $script:Amber
    Write-Host "    Ensure .env is in .gitignore and NEVER committed." -ForegroundColor $script:Amber
    Write-Host "    See: https://tauri.app/start/prerequisites/#signing" -ForegroundColor $script:Muted
    exit 0
}

if ($Build) {
    Write-Host ""
    # Auto-update artifacts (.sig + latest.json) are only emitted when the
    # Ed25519 updater signing key is present. Without it the installer still
    # builds and installs fine -- it just can't push future auto-updates.
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
        Write-Warn "TAURI_SIGNING_PRIVATE_KEY not set -- the installer will build but WON'T be auto-updatable."
        Write-Info "to enable auto-update, generate a key once:"
        Write-Info "    npx @tauri-apps/cli signer generate -w `"$env:USERPROFILE\.tauri\kilroy_updater.key`""
        Write-Info "then set TAURI_SIGNING_PRIVATE_KEY (+ _PASSWORD) and paste the public key into src-tauri/tauri.conf.json (plugins.updater.pubkey)."
        Write-Info "Store the key file OUTSIDE the repo directory (e.g. `"$env:USERPROFILE\.tauri\kilroy_updater.key`")."
    } else {
        Write-Ok "updater signing key detected -- release will include signed update artifacts"
    }
    Write-Host "  Building the consumer installer via build:release (doctor + fetch Ollama + build)..." -ForegroundColor $script:Amber
    Write-Host "  First build compiles ~200 crates -- give it 10-20 minutes." -ForegroundColor $script:Muted
    & npm run build:release
} else {
    Write-Host ""
    Write-Host "  Starting Kilroy in dev mode." -ForegroundColor $script:Amber
    Write-Host "  First build compiles ~150 crates -- give it 8-15 minutes." -ForegroundColor $script:Muted
    Write-Host ""
    & npm run tauri:dev
}