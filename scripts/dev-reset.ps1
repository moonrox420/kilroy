[CmdletBinding()]
param([switch]$Rust, [switch]$StopOnly, [switch]$DryRun)

$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if (-not (Test-Path -LiteralPath (Join-Path $projectRoot 'src-tauri/Cargo.toml') -PathType Leaf)) {
    throw 'Could not verify the Kilroy project root.'
}
$prefix = $projectRoot.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar

if ($Rust -or $StopOnly) {
    # Only the application binary from this checkout is in scope, never all Node/WebView processes.
    $processes = Get-CimInstance Win32_Process -Filter "Name = 'kilroy.exe'"
    foreach ($process in $processes) {
        if ($process.ExecutablePath -and $process.ExecutablePath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
            if ($DryRun) { Write-Host "Would stop $($process.ExecutablePath) (PID $($process.ProcessId))"; continue }
            & taskkill.exe /F /T /PID $process.ProcessId
            if ($LASTEXITCODE -ne 0) { throw "Could not stop Kilroy PID $($process.ProcessId)" }
        }
    }
}
if ($StopOnly) { return }

$cache = [IO.Path]::GetFullPath((Join-Path $projectRoot 'node_modules/.vite'))
if (-not $cache.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Cache target escaped the project.' }
foreach ($candidate in @($projectRoot, (Join-Path $projectRoot 'node_modules'), $cache)) {
    if (Test-Path -LiteralPath $candidate) {
        if ((Get-Item -LiteralPath $candidate -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "Refusing cache cleanup through a reparse point: $candidate"
        }
    }
}
if ($DryRun) {
    Write-Host "Would clear only $cache"
    if ($Rust) { Write-Host 'Would run cargo clean --package kilroy for this checkout.' }
    return
}
if (Test-Path -LiteralPath $cache) { Remove-Item -LiteralPath $cache -Recurse -Force }
if ($Rust) {
    & cargo clean --manifest-path (Join-Path $projectRoot 'src-tauri/Cargo.toml') --package kilroy
    if ($LASTEXITCODE -ne 0) { throw 'Scoped Rust clean failed.' }
}
