# Direct shim for force-uninstalling Kilroy from any PowerShell prompt.
# Bypasses npm. Use this if `npm run wipe:force` is being weird.
#
# Usage:
#   .\wipe.ps1            # force uninstall + WebView2 ghost kill
#   .\wipe.ps1 -Nuke      # also wipe build artifacts + project data
[CmdletBinding()]
param(
  [switch]$Nuke
)
$script = Join-Path $PSScriptRoot 'scripts\force-uninstall.ps1'
if ($Nuke) {
  & $script -IncludeBuild -IncludeProjectData
} else {
  & $script
}
