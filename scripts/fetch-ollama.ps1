<#
.SYNOPSIS
  Bundle the repository-pinned Ollama release after SHA-256 verification.
.PARAMETER Version
  Optional assertion of the version in ollama-release.json. Mutable latest tags are rejected.
.PARAMETER Force
  Download a fresh archive even when a verified cache exists.
.PARAMETER VerifyOnly
  Verify the installed bundle and its per-file manifest without downloading.
#>
[CmdletBinding()]
param([string]$Version, [switch]$Force, [switch]$VerifyOnly)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'artifact-integrity.ps1')
$projectRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$release = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'ollama-release.json') -Raw | ConvertFrom-Json
if ($Version -and $Version -ne $release.version) { throw "Requested version must match the checked-in lock ($($release.version)). Update the lock and verified digest together." }
if ($release.url -ne "https://github.com/ollama/ollama/releases/download/$($release.version)/$($release.asset)") { throw 'Release URL does not match the locked official GitHub asset.' }
$destination = Join-Path $projectRoot 'src-tauri\resources\ollama'
$manifestPath = Join-Path $destination '.kilroy-bundle.json'

function Test-InstalledBundle {
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { return $false }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.version -ne $release.version -or $manifest.archiveSha256 -ne $release.sha256) { return $false }
    $prefix = [IO.Path]::GetFullPath($destination).TrimEnd('\') + '\'
    foreach ($file in $manifest.files) {
        $path = [IO.Path]::GetFullPath((Join-Path $destination $file.path))
        if (-not $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid bundle manifest path.' }
        Assert-ArtifactHash -Path $path -Sha256 $file.sha256
    }
    $actualCount = @(Get-ChildItem -LiteralPath $destination -File -Recurse -Force | Where-Object { $_.Name -ne '.kilroy-bundle.json' }).Count
    return $actualCount -eq @($manifest.files).Count -and (Test-Path -LiteralPath (Join-Path $destination 'ollama.exe'))
}

if (-not $Force -and (Test-InstalledBundle)) { Write-Host "Verified bundled Ollama $($release.version)."; exit 0 }
if ($VerifyOnly) { throw 'A complete verified Ollama bundle is not installed.' }
if ((Test-Path -LiteralPath $destination) -and ((Get-Item -LiteralPath $destination -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Ollama resource directory must not be a symlink or junction.' }

$cache = Join-Path $projectRoot '.kilroy\release-cache'
New-Item -ItemType Directory -Force -Path $cache | Out-Null
$archive = Join-Path $cache "$($release.version)-$($release.asset)"
if ($Force -or -not (Test-Path -LiteralPath $archive -PathType Leaf)) {
    $partial = "$archive.$([guid]::NewGuid().ToString('N')).partial"
    Write-Host "Downloading pinned Ollama $($release.version) ($([math]::Round($release.size / 1MB)) MiB)."
    Invoke-WebRequest -Uri $release.url -OutFile $partial -UseBasicParsing
    Assert-ArtifactHash -Path $partial -Sha256 $release.sha256
    Move-Item -LiteralPath $partial -Destination $archive -Force
}
Assert-ArtifactHash -Path $archive -Sha256 $release.sha256
$staging = Join-Path $cache "extract-$([guid]::NewGuid().ToString('N'))"
Assert-SafeArchive -Archive $archive -Destination $staging
[IO.Compression.ZipFile]::ExtractToDirectory($archive, $staging)
if (-not (Test-Path -LiteralPath (Join-Path $staging 'ollama.exe') -PathType Leaf)) { throw "The verified archive has no root ollama.exe. Extracted files remain at $staging." }
$prefixLength = $staging.TrimEnd('\').Length + 1
$files = @(Get-ChildItem -LiteralPath $staging -File -Recurse -Force | ForEach-Object {
    [ordered]@{ path = $_.FullName.Substring($prefixLength); sha256 = Get-ArtifactHash -Path $_.FullName }
})
$manifest = [ordered]@{ version = $release.version; archiveSha256 = $release.sha256; files = $files }
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $staging '.kilroy-bundle.json') -Encoding UTF8

$backup = $null
if (Test-Path -LiteralPath $destination) {
    $backup = Join-Path $cache "previous-$([guid]::NewGuid().ToString('N'))"
    if (-not ([IO.Path]::GetFullPath($destination).StartsWith($projectRoot + '\', [StringComparison]::OrdinalIgnoreCase))) { throw 'Resource directory escaped project root.' }
    Move-Item -LiteralPath $destination -Destination $backup
}
try { Move-Item -LiteralPath $staging -Destination $destination }
catch {
    if ($backup -and -not (Test-Path -LiteralPath $destination)) { Move-Item -LiteralPath $backup -Destination $destination }
    throw
}
if (-not (Test-InstalledBundle)) { throw 'Installed bundle failed final manifest verification.' }
Write-Host "Verified Ollama $($release.version) installed at $destination."
if ($backup) { Write-Host "Previous bundle retained at $backup." }
