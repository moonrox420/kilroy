<#
.SYNOPSIS
  Regenerate the bundled tray / app icons (.ico, .icns, .png set) from
  the canonical SVG source at src-tauri/icons/icon.svg.

.DESCRIPTION
  Tauri's `tauri icon` command takes a PNG source and generates every
  platform-specific icon size. We rasterize the SVG to icon.png at
  1024x1024 first, then hand it off.

  Prefers ImageMagick (winget install ImageMagick.ImageMagick) for
  rasterization. Falls back to Inkscape if that's on PATH. Errors with
  a helpful install hint otherwise.

.EXAMPLE
  npm run icons:regen
  Rasterizes icon.svg, regenerates 32x32.png, 128x128.png,
  128x128@2x.png, icon.icns, icon.ico.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$svg = "src-tauri\icons\icon.svg"
$png = "src-tauri\icons\icon.png"

if (-not (Test-Path -LiteralPath $svg)) {
  Write-Error "Source SVG not found: $svg"
  exit 1
}

Write-Host ""
Write-Host "=== Kilroy icon regeneration ===" -ForegroundColor Cyan
Write-Host ""

# Step 1 -- SVG to PNG. Try ImageMagick, then Inkscape, then fail loud.
function Find-Cli {
  param([string]$Name)
  $cmd = Get-Command $Name -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source } else { return $null }
}

$magick = Find-Cli 'magick'
$inkscape = Find-Cli 'inkscape'

if ($magick) {
  Write-Host "[1/2] Rasterising via ImageMagick..." -ForegroundColor Cyan
  & $magick -background none -density 384 $svg -resize 1024x1024 $png
} elseif ($inkscape) {
  Write-Host "[1/2] Rasterising via Inkscape..." -ForegroundColor Cyan
  & $inkscape --export-type=png --export-filename=$png --export-width=1024 $svg
} else {
  Write-Host "No SVG rasteriser found on PATH." -ForegroundColor Red
  Write-Host ""
  Write-Host "Install one of:" -ForegroundColor Yellow
  Write-Host "  winget install ImageMagick.ImageMagick"
  Write-Host "  winget install Inkscape.Inkscape"
  Write-Host ""
  Write-Host "Or rasterise icon.svg -> icon.png manually (1024x1024) and re-run." -ForegroundColor Yellow
  exit 1
}

if (-not (Test-Path -LiteralPath $png)) {
  Write-Error "Rasterisation appears to have failed -- $png was not produced."
  exit 1
}
Write-Host "  produced $png" -ForegroundColor Green

# Step 2 -- hand off to Tauri's icon generator. Outputs every size /
# format into src-tauri/icons/.
Write-Host "[2/2] Generating platform icons via Tauri CLI..." -ForegroundColor Cyan
& npx @tauri-apps/cli icon $png -o "src-tauri\icons"

Write-Host ""
Write-Host "Done. Updated icons:" -ForegroundColor Green
Get-ChildItem "src-tauri\icons" -Filter "icon*" |
  ForEach-Object { Write-Host "  $($_.Name)" -ForegroundColor DarkGray }
Get-ChildItem "src-tauri\icons" -Filter "*x*.png" |
  ForEach-Object { Write-Host "  $($_.Name)" -ForegroundColor DarkGray }
Write-Host ""
