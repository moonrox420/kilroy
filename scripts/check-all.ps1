[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$expectedPackage = Join-Path $projectRoot "package.json"
if (-not (Test-Path -LiteralPath $expectedPackage -PathType Leaf)) {
    throw "Kilroy project root could not be resolved from $PSScriptRoot"
}

Set-Location -LiteralPath $projectRoot

function Invoke-Checked {
    param(
        [Parameter(Mandatory)]
        [string]$Label,
        [Parameter(Mandatory)]
        [scriptblock]$Command
    )

    Write-Host "==> $Label" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE"
    }
}

$testTempParent = Join-Path $projectRoot ".kilroy\test-tmp"
New-Item -ItemType Directory -Force -Path $testTempParent | Out-Null

Invoke-Checked "Frontend build, test typecheck, and tests" { npm run check }
Invoke-Checked "Rust formatting" { cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check }
Invoke-Checked "Rust lint" { cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings }
Invoke-Checked "Rust tests" { cargo test --manifest-path src-tauri/Cargo.toml --locked }
Invoke-Checked "Release artifact integrity tests" { powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-artifact-integrity.ps1 }

Write-Host "All Kilroy verification gates passed." -ForegroundColor Green
