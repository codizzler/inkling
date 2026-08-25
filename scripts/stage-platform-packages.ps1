# Assemble the per-platform npm packages locally from a CI build, so they can be
# published by hand. Mirrors exactly what .github/workflows/npm.yml does.
#
#   .\scripts\stage-platform-packages.ps1 [run-id]
#
# With no run id it uses the most recent successful npm workflow run. The
# binaries are whatever CI built, and nothing is compiled here, so this works
# from any machine regardless of what it can cross compile.
#
# The POSIX twin of this script is stage-platform-packages.sh; keep them in step.

param([string]$RunId)

# See publish-platform-packages.ps1: 'Stop' plus native stderr is a trap in
# PowerShell 5.1, so exit codes are checked explicitly.
$ErrorActionPreference = 'Continue'

$root = Split-Path -Parent $PSScriptRoot
Set-Location (Join-Path $root 'crates\inkling-node')

if ([string]::IsNullOrWhiteSpace($RunId)) {
    $RunId = gh run list --workflow npm.yml --status success --limit 1 --json databaseId -q '.[0].databaseId'
    Write-Host "using the last successful npm run: $RunId"
}

foreach ($stale in @('artifacts', 'npm')) {
    if (Test-Path $stale) { Remove-Item -Recurse -Force $stale }
}

foreach ($step in @(
        { gh run download $RunId -D artifacts },
        { npm install --silent },
        { npx napi create-npm-dir --target . },
        { npx napi artifacts })) {
    & $step
    if ($LASTEXITCODE -ne 0) {
        Write-Host "failed: $step" -ForegroundColor Red
        exit 1
    }
}

Write-Host ""
foreach ($dir in Get-ChildItem -Directory 'npm') {
    $pkg = Get-Content (Join-Path $dir.FullName 'package.json') -Raw | ConvertFrom-Json
    $binary = Get-ChildItem -Path $dir.FullName -Filter '*.node' -ErrorAction SilentlyContinue
    if ($binary) {
        Write-Host ("staged  {0}" -f $pkg.name)
    }
    else {
        Write-Host ("MISSING {0} has no binary" -f $pkg.name) -ForegroundColor Red
    }
}
