<#
.SYNOPSIS
  Download Ollama's Windows binary into src-tauri/resources/ollama/ so
  it gets bundled into the NSIS installer at tauri:build time.

.DESCRIPTION
  Pulls the latest Windows release from github.com/ollama/ollama and
  extracts to src-tauri/resources/ollama/. The tauri.conf.json's
  bundle.resources picks up this folder and bakes it into the
  installer.

  Idempotent -- re-runs check the existing version and skip download if
  current. Pass -Force to re-download regardless.

.PARAMETER Version
  Specific Ollama release tag (e.g. "v0.5.13"). Default: latest.

.PARAMETER Force
  Download even if a binary already exists in resources/ollama/.

.EXAMPLE
  npm run fetch:ollama
  Downloads latest Ollama into the resources folder.

.EXAMPLE
  .\scripts\fetch-ollama.ps1 -Version v0.5.13 -Force
  Pin to a specific version, overwriting any existing binary.
#>

[CmdletBinding()]
param(
  [string]$Version = "latest",
  [switch]$Force
)

$ErrorActionPreference = 'Stop'

$dest = "src-tauri\resources\ollama"
$exe = "$dest\ollama.exe"

if (-not (Test-Path $dest)) {
  New-Item -ItemType Directory -Path $dest -Force | Out-Null
}

if ((Test-Path $exe) -and (-not $Force)) {
  $size = (Get-Item $exe).Length
  Write-Host "Ollama already present at $exe ($([math]::Round($size/1MB,1)) MB). Use -Force to re-download." -ForegroundColor DarkGray
  exit 0
}

# Resolve release URL
$releaseApi = if ($Version -eq "latest") {
  "https://api.github.com/repos/ollama/ollama/releases/latest"
} else {
  "https://api.github.com/repos/ollama/ollama/releases/tags/$Version"
}

Write-Host "Resolving Ollama release ($Version)..." -ForegroundColor Cyan
$headers = @{ "User-Agent" = "Kilroy-build" }
$rel = Invoke-RestMethod -Uri $releaseApi -Headers $headers

# The asset name has shifted over Ollama versions. Match by common
# Windows x86_64 patterns.
$asset = $rel.assets | Where-Object {
  $_.name -match 'windows.*amd64.*\.zip$' -or
  $_.name -match 'windows.*x86_64.*\.zip$' -or
  $_.name -eq 'ollama-windows.zip'
} | Select-Object -First 1

if (-not $asset) {
  Write-Host "Could not find a Windows x86_64 .zip asset in this release." -ForegroundColor Red
  Write-Host "Available assets:" -ForegroundColor Yellow
  $rel.assets | ForEach-Object { Write-Host "  $($_.name)" }
  exit 1
}

$url = $asset.browser_download_url
$zip = "$env:TEMP\ollama-$($rel.tag_name).zip"

Write-Host "Downloading $($asset.name) ($([math]::Round($asset.size/1MB,1)) MB)..." -ForegroundColor Cyan
Write-Host "  from: $url" -ForegroundColor DarkGray
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing

Write-Host "Extracting to $dest..." -ForegroundColor Cyan
# Clear destination first to avoid stale files from prior versions.
Get-ChildItem $dest -Exclude '.gitkeep' | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
Expand-Archive -Path $zip -DestinationPath $dest -Force
Remove-Item $zip -Force

if (-not (Test-Path $exe)) {
  # Some Ollama versions ship the binary nested. Hoist it up.
  $found = Get-ChildItem -Path $dest -Recurse -Filter 'ollama.exe' | Select-Object -First 1
  if ($found) {
    Move-Item $found.FullName $exe -Force
    Write-Host "Hoisted ollama.exe from nested path." -ForegroundColor DarkGray
  } else {
    Write-Host "Extraction succeeded but ollama.exe was not found in the archive." -ForegroundColor Red
    exit 1
  }
}

$finalSize = (Get-Item $exe).Length
Write-Host ""
Write-Host "Done. Bundled Ollama:" -ForegroundColor Green
Write-Host "  version: $($rel.tag_name)" -ForegroundColor Green
Write-Host "  path:    $exe" -ForegroundColor Green
Write-Host "  size:    $([math]::Round($finalSize/1MB,1)) MB" -ForegroundColor Green
Write-Host ""
Write-Host "Next: npm run tauri:build" -ForegroundColor Yellow
