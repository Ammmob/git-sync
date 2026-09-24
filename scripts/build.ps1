param([switch]$Installer)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$cargoRoot = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$env:PATH = (Join-Path $cargoRoot 'bin') + ';' + $env:PATH
Set-Location -LiteralPath $projectRoot
if ($Installer) { & npm.cmd run desktop:build } else { & npm.cmd run desktop:build -- --no-bundle }
if ($LASTEXITCODE -ne 0) { throw 'Desktop build failed' }
$bin = Join-Path $projectRoot '.local\bin'
New-Item -ItemType Directory -Force -Path $bin | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot 'src-tauri\target\release\git-sync-desktop.exe') -Destination $bin -Force
Write-Output ('Executable: ' + (Join-Path $bin 'git-sync-desktop.exe'))
