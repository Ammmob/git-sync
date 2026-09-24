$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $projectRoot '.local\bin\git-sync-desktop.exe'
if (-not (Test-Path -LiteralPath $exe)) { $exe = Join-Path $projectRoot 'src-tauri\target\release\git-sync-desktop.exe' }
if (-not (Test-Path -LiteralPath $exe)) { throw 'Build the desktop app first: powershell -ExecutionPolicy Bypass -File scripts\build.ps1' }
Start-Process -FilePath $exe -WorkingDirectory $projectRoot -WindowStyle Hidden
