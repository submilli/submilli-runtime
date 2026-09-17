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
    $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/submilli/submilli-runtime/releases/latest' -Headers $headers
    $Version = $release.tag_name
}
if ($Version -notmatch '^[a-zA-Z0-9_][a-zA-Z0-9._-]*$') { throw 'Invalid release tag.' }
$binaries = @('submilli', 'submilli-server')
$base = "https://github.com/submilli/submilli-runtime/releases/download/$Version"
$scratch = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
$stagedFiles = @()
New-Item -ItemType Directory -Path $scratch | Out-Null
try {
    $checksums = Join-Path $scratch 'SHA256SUMS'
    Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -Headers $headers -OutFile $checksums
    # Verify every executable before installing any, so a failure changes nothing.
    foreach ($name in $binaries) {
        $asset = "$name-x86_64-pc-windows-msvc.exe"
        $binary = Join-Path $scratch $asset
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -Headers $headers -OutFile $binary
        $matchesForAsset = @(Get-Content -LiteralPath $checksums | Where-Object { $_ -match ('^[0-9a-fA-F]{64}\s+' + [regex]::Escape($asset) + '$') })
        if ($matchesForAsset.Count -ne 1) { throw "Missing or ambiguous checksum for $asset" }
        $expected = ($matchesForAsset[0] -split '\s+')[0]
        if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash -ne $expected) {
            throw 'Checksum mismatch; existing installation was not changed.'
        }
        & $binary --version
        if ($LASTEXITCODE -ne 0) { throw "Downloaded $name failed its version check." }
    }
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    foreach ($name in $binaries) {
        if (Test-Path -LiteralPath (Join-Path $InstallDir "$name.exe") -PathType Container) { throw "Installation target $name.exe is a directory." }
    }
    foreach ($name in $binaries) {
        $staged = Join-Path $InstallDir (".$name-" + [Guid]::NewGuid().ToString() + '.exe')
        $stagedFiles += $staged
        Copy-Item -LiteralPath (Join-Path $scratch "$name-x86_64-pc-windows-msvc.exe") -Destination $staged
    }
    for ($i = 0; $i -lt $binaries.Count; $i++) {
        $destination = Join-Path $InstallDir ($binaries[$i] + '.exe')
        if (Test-Path -LiteralPath $destination) {
            [IO.File]::Replace($stagedFiles[$i], $destination, [NullString]::Value)
        } else {
            [IO.File]::Move($stagedFiles[$i], $destination)
        }
    }
    Write-Host "Installed submilli and submilli-server $Version to $InstallDir"
    if ($InstallDir -notin ($env:PATH -split ';')) {
        Write-Host "Add this directory to your user PATH: $InstallDir"
    }
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
    foreach ($staged in $stagedFiles) {
        if (Test-Path -LiteralPath $staged) { Remove-Item -LiteralPath $staged -Force }
    }
}
