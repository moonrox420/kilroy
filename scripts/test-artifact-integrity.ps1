$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'artifact-integrity.ps1')
$projectRoot = Split-Path -Parent $PSScriptRoot
$testRoot = Join-Path $projectRoot ".kilroy\test-tmp\artifact-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force -Path $testRoot | Out-Null
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem

function Assert-Rejected {
    param([scriptblock]$Operation, [string]$Label)
    $rejected = $false
    try { & $Operation } catch { $rejected = $true }
    if (-not $rejected) { throw "Expected rejection: $Label" }
}

try {
    $file = Join-Path $testRoot 'data.txt'
    [IO.File]::WriteAllText($file, 'abc')
    $digest = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'
    Assert-ArtifactHash -Path $file -Sha256 $digest
    Assert-Rejected { Assert-ArtifactHash -Path $file -Sha256 ('0' * 64) } 'modified artifact'
    Assert-Rejected { Assert-ArtifactHash -Path $file -Sha256 'missing-digest' } 'invalid digest'
    $valid = Join-Path $testRoot 'valid.zip'
    $archive = [IO.Compression.ZipFile]::Open($valid, [IO.Compression.ZipArchiveMode]::Create)
    try { $archive.CreateEntry('nested/file.txt') | Out-Null } finally { $archive.Dispose() }
    Assert-SafeArchive -Archive $valid -Destination (Join-Path $testRoot 'extract')
    foreach ($unsafe in @('../escape.txt', 'file.txt:stream', 'nested/../../escape.txt')) {
        $path = Join-Path $testRoot "$([guid]::NewGuid().ToString('N')).zip"
        $archive = [IO.Compression.ZipFile]::Open($path, [IO.Compression.ZipArchiveMode]::Create)
        try { $archive.CreateEntry($unsafe) | Out-Null } finally { $archive.Dispose() }
        Assert-Rejected { Assert-SafeArchive -Archive $path -Destination (Join-Path $testRoot 'extract') } $unsafe
    }
    Write-Host 'Artifact integrity checks passed: valid digest/archive, tampering, invalid digest, traversal, and alternate streams.'
} finally {
    $resolved = [IO.Path]::GetFullPath($testRoot)
    $allowed = [IO.Path]::GetFullPath((Join-Path $projectRoot '.kilroy\test-tmp')).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($allowed, [StringComparison]::OrdinalIgnoreCase)) { throw 'Test cleanup path escaped test root.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
