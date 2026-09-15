# Usage: .\install.ps1 [-Version v0.1.0] [-InstallDir "$env:LOCALAPPDATA\Submilli\bin"]
[CmdletBinding()]
param(
    [string]$Version = 'latest',
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Submilli\bin')
)
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'Use install.sh on macOS or Linux.'
}
$architecture = [Environment]::GetEnvironmentVariable('PROCESSOR_ARCHITEW6432')
if (-not $architecture) { $architecture = $env:PROCESSOR_ARCHITECTURE }
if ($architecture -ne 'AMD64') { throw 'The Windows release supports x86_64 only.' }
if ([string]::IsNullOrWhiteSpace($InstallDir)) { throw 'Install directory must not be empty.' }
$InstallDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($InstallDir)
# Windows PowerShell 5.1 may otherwise negotiate an obsolete TLS version.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$headers = @{ 'User-Agent' = 'submilli-installer' }
if ($Version -eq 'latest') {
    $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/submilli/submilli-public/releases/latest' -Headers $headers
    $Version = $release.tag_name
}
if ($Version -notmatch '^[a-zA-Z0-9_][a-zA-Z0-9._-]*$') { throw 'Invalid release tag.' }
$asset = 'submilli-x86_64-pc-windows-msvc.exe'
$base = "https://github.com/submilli/submilli-public/releases/download/$Version"
$scratch = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
$staged = $null
New-Item -ItemType Directory -Path $scratch | Out-Null
try {
    $binary = Join-Path $scratch $asset
    $checksums = Join-Path $scratch 'SHA256SUMS'
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -Headers $headers -OutFile $binary
    Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -Headers $headers -OutFile $checksums
    $matchesForAsset = @(Get-Content -LiteralPath $checksums | Where-Object { $_ -match ('^[0-9a-fA-F]{64}\s+' + [regex]::Escape($asset) + '$') })
    if ($matchesForAsset.Count -ne 1) { throw "Missing or ambiguous checksum for $asset" }
    $expected = ($matchesForAsset[0] -split '\s+')[0]
    if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash -ne $expected) {
        throw 'Checksum mismatch; existing installation was not changed.'
    }
    & $binary --version
    if ($LASTEXITCODE -ne 0) { throw 'Downloaded executable failed its version check.' }
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $destination = Join-Path $InstallDir 'submilli.exe'
    if (Test-Path -LiteralPath $destination -PathType Container) { throw 'Installation target is a directory.' }
    $staged = Join-Path $InstallDir ('.submilli-' + [Guid]::NewGuid().ToString() + '.exe')
    Copy-Item -LiteralPath $binary -Destination $staged
    if (Test-Path -LiteralPath $destination) {
        [IO.File]::Replace($staged, $destination, [NullString]::Value)
    } else {
        [IO.File]::Move($staged, $destination)
    }
    $staged = $null
    Write-Host "Installed $Version to $destination"
    if ($InstallDir -notin ($env:PATH -split ';')) {
        Write-Host "Add this directory to your user PATH: $InstallDir"
    }
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
    if ($staged -and (Test-Path -LiteralPath $staged)) { Remove-Item -LiteralPath $staged -Force }
}
