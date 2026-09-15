# Run with pwsh on Windows. Downloads use fixtures; no release or PATH is modified.
$ErrorActionPreference = 'Stop'
$installer = Join-Path $PSScriptRoot '../../install.ps1'
$root = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $root | Out-Null
$destination = Join-Path $root 'install with spaces'
$fixture = Join-Path $root 'fixture.exe'
# A real standalone Windows executable that supports --version.
Copy-Item (Get-Command curl.exe).Source $fixture
$checksum = (Get-FileHash $fixture -Algorithm SHA256).Hash
$fixtureState = @{ BadChecksum = $false; FailDownload = $false }
function Invoke-RestMethod { param($Uri, $Headers) return @{ tag_name = 'v9.8.7' } }
function Invoke-WebRequest {
    param([switch]$UseBasicParsing, $Uri, $Headers, $OutFile)
    if ($fixtureState.FailDownload) { throw 'Simulated download failure' }
    if ($Uri.EndsWith('/SHA256SUMS')) {
        $hash = if ($fixtureState.BadChecksum) { '0' * 64 } else { $checksum }
        Set-Content -LiteralPath $OutFile -Value "$hash  submilli-x86_64-pc-windows-msvc.exe" -Encoding ascii
    } else {
        Copy-Item -LiteralPath $fixture -Destination $OutFile
    }
}
function Assert-InstallFails {
    param([string]$Version = 'v9.8.7')
    $failed = $false
    try { & $installer -Version $Version -InstallDir $destination } catch { $failed = $true }
    if (-not $failed) { throw 'Expected installer failure' }
    if ((Get-FileHash (Join-Path $destination 'submilli.exe')).Hash -ne $checksum) {
        throw 'Failed installation changed the existing executable'
    }
}
try {
    & $installer -InstallDir $destination
    & $installer -Version v9.8.7 -InstallDir $destination
    if ((Get-FileHash (Join-Path $destination 'submilli.exe')).Hash -ne $checksum) { throw 'Installed bytes differ' }
    $fixtureState.BadChecksum = $true
    Assert-InstallFails
    $fixtureState.BadChecksum = $false
    $fixtureState.FailDownload = $true
    Assert-InstallFails
    $fixtureState.FailDownload = $false
    Assert-InstallFails -Version '../main'
    Write-Host 'Windows install, upgrade, checksum, download-failure, and invalid-version checks passed.'
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force
}
