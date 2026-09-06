function Get-ArtifactHash {
    param([Parameter(Mandatory)][string]$Path)
    $stream = [IO.File]::OpenRead($Path)
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}

function Assert-ArtifactHash {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Sha256)
    if ($Sha256 -notmatch '^[0-9a-fA-F]{64}$') { throw 'Expected SHA-256 must contain exactly 64 hex digits.' }
    $actual = Get-ArtifactHash -Path $Path
    if ($actual -ne $Sha256) { throw "Integrity check failed for $Path. Expected $Sha256; got $actual. Nothing was extracted." }
}

function Assert-SafeArchive {
    param([Parameter(Mandatory)][string]$Archive, [Parameter(Mandatory)][string]$Destination)
    Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
    $root = [IO.Path]::GetFullPath($Destination).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $total = 0L
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        if ($zip.Entries.Count -gt 20000) { throw 'Archive exceeds the file-count limit.' }
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName.Replace('/', [IO.Path]::DirectorySeparatorChar)
            $target = [IO.Path]::GetFullPath((Join-Path $root $name))
            if ($name.Contains(':') -or -not $target.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {
                throw "Unsafe archive path: $($entry.FullName)"
            }
            if (-not $names.Add($target)) { throw "Duplicate archive path: $($entry.FullName)" }
            if ((($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000) { throw 'Archive symlinks are not permitted.' }
            $total += $entry.Length
            if ($total -gt 8GB) { throw 'Archive exceeds the 8 GiB expanded-size limit.' }
        }
    } finally { $zip.Dispose() }
}
