# One-time bootstrap: publish the seven per-platform npm packages by hand.
#
#   .\scripts\stage-platform-packages.ps1     # assemble from a CI build
#   npm login                                 # once, if not already logged in
#   .\scripts\publish-platform-packages.ps1   # prompts for an OTP per package
#
# Why this is manual. The Node addon ships one npm package per platform, and the
# main package depends on all of them optionally. npm trusted publishing cannot
# create a package that does not exist yet, there being no pending-publisher
# flow as there is on PyPI, and granular tokens with Bypass 2FA are now
# restricted for direct publishing, so CI cannot create these names either.
#
# Once the names exist this is never needed again: CI only ever publishes new
# *versions* of packages that are already there, which an ordinary token can do.
#
# The POSIX twin of this script is publish-platform-packages.sh; keep them in
# step.

# Not 'Stop'. npm writes to stderr on the perfectly normal "this version is not
# published yet" path, and PowerShell 5.1 wraps native stderr in an ErrorRecord,
# so 'Stop' aborts the script on the case this script exists to handle. Exit
# codes are checked explicitly instead.
$ErrorActionPreference = 'Continue'

$root = Split-Path -Parent $PSScriptRoot
Set-Location (Join-Path $root 'crates\inkling-node')

if (-not (Test-Path 'npm')) {
    Write-Host "npm/ is not staged; run .\scripts\stage-platform-packages.ps1 first" -ForegroundColor Red
    exit 1
}

$published = 0
$skipped = 0
$outstanding = 0

foreach ($dir in Get-ChildItem -Directory 'npm') {
    $manifest = Join-Path $dir.FullName 'package.json'
    $pkg = Get-Content $manifest -Raw | ConvertFrom-Json
    $name = $pkg.name
    $version = $pkg.version

    $binary = Get-ChildItem -Path $dir.FullName -Filter '*.node' -ErrorAction SilentlyContinue
    if (-not $binary) {
        Write-Host "SKIP  $name has no binary staged" -ForegroundColor Yellow
        $outstanding++
        continue
    }

    # `npm view` exits non-zero when the version is not on the registry, which is
    # the normal case here, so the exit code is inspected rather than trusted to
    # be zero.
    npm view "$name@$version" version 2>$null | Out-Null
    $alreadyPublished = ($LASTEXITCODE -eq 0)
    $global:LASTEXITCODE = 0
    if ($alreadyPublished) {
        Write-Host "ok    $name@$version is already published" -ForegroundColor DarkGray
        $skipped++
        continue
    }

    Write-Host ""
    Write-Host "$name@$version" -ForegroundColor Cyan
    # A code from your authenticator is the quickest path, but npm can also do
    # the whole thing in a browser, which avoids racing a 30 second TOTP window
    # seven times in a row.
    $otp = Read-Host 'authenticator code (or press Enter to authenticate in your browser)'

    Push-Location $dir.FullName
    try {
        if ([string]::IsNullOrWhiteSpace($otp)) {
            npm publish --access public
        }
        else {
            npm publish --access public --otp $otp
        }
        if ($LASTEXITCODE -eq 0) {
            Write-Host "ok    published $name@$version" -ForegroundColor Green
            $published++
        }
        else {
            Write-Host "FAIL  $name@$version" -ForegroundColor Red
            $outstanding++
        }
    }
    finally {
        Pop-Location
    }
}

Write-Host ""
Write-Host "$published published, $skipped already there, $outstanding still outstanding"
Write-Host 're-running is safe: anything already on the registry is skipped'
if ($outstanding -ne 0) { exit 1 }
