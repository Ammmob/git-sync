$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$cargoRoot = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$env:PATH = (Join-Path $cargoRoot 'bin') + ';' + $env:PATH
Set-Location -LiteralPath $projectRoot
& npm.cmd run desktop:dev
exit $LASTEXITCODE
