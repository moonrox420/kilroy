@echo off
REM Direct shim for force-uninstalling Kilroy. Bypasses npm entirely.
REM Usage:  wipe          (force uninstall + WebView2 ghost kill)
REM         wipe nuke     (also wipe build artifacts + project data)
setlocal
set "SCRIPT=%~dp0scripts\force-uninstall.ps1"
if /i "%~1"=="nuke" (
  powershell -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT%" -IncludeBuild -IncludeProjectData
) else (
  powershell -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT%"
)
endlocal
