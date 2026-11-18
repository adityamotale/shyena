[CmdletBinding()]
param(
    [string]$Version = 'latest',
    [switch]$NoLaunch,
    [switch]$NoAutostart,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repoOwner = 'adityamotale'
$repoName = 'shyena'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\shyena'
$exeTarget = Join-Path $installDir 'shyena.exe'
$configDir = Join-Path $env:USERPROFILE '.config'
$configTarget = Join-Path $configDir '.shyena'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

function Assert-ShyenaEnvironment {
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw 'Shyena requires a 64-bit Windows installation.'
    }

    $build = [int](Get-CimInstance -ClassName Win32_OperatingSystem).BuildNumber
    if ($build -lt 19041) {
        throw "Shyena requires Windows 10 build 19041 or newer (detected build $build)."
    }

    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Shyena must be installed from a non-elevated shell (standard user token).'
    }

    if ($PSVersionTable.PSVersion.Major -lt 5) {
        throw 'Shyena requires Windows PowerShell 5.1 or newer.'
    }
}

function Get-ShyenaReleaseBase {
    if ([string]::IsNullOrWhiteSpace($Version) -or $Version -eq 'latest') {
        return "https://github.com/$repoOwner/$repoName/releases/latest/download"
    }
    return "https://github.com/$repoOwner/$repoName/releases/download/$Version"
}

function Save-ShyenaAsset {
    param(
        [string]$Name,
        [string]$BaseUrl,
        [string]$Destination
    )

    $uri = "$BaseUrl/$Name"
    $request = @{
        Uri = $uri
        OutFile = $Destination
        TimeoutSec = 120
    }
    if ($PSVersionTable.PSVersion.Major -lt 6) {
        $request['UseBasicParsing'] = $true
    }

    try {
        Invoke-WebRequest @request
    } catch {
        throw "Could not download $Name from $uri`: $($_.Exception.Message)"
    }
}

function Read-ShyenaChecksums {
    param([string]$Path)

    $map = @{}
    foreach ($line in [IO.File]::ReadAllLines($Path)) {
        if ($line -match '^([0-9a-fA-F]{64})\s+(\S+)\s*$') {
            $map[$Matches[2]] = $Matches[1].ToLowerInvariant()
        }
    }
    return $map
}

function Test-ShyenaChecksums {
    param(
        [string]$Directory,
        [hashtable]$Expected
    )

    foreach ($name in @('shyena.exe', 'default.shyena')) {
        if (-not $Expected.ContainsKey($name)) {
            throw "SHA256SUMS does not contain an entry for $name."
        }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $Directory $name)).Hash.ToLowerInvariant()
        if ($actual -ne $Expected[$name]) {
            throw "Checksum mismatch for $name (expected $($Expected[$name]), got $actual)."
        }
    }
}

function Add-ShyenaToUserPath {
    param([string]$Directory)

    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ([string]::IsNullOrEmpty($current)) {
        [Environment]::SetEnvironmentVariable('Path', $Directory, 'User')
        return $true
    }

    $target = $Directory.Trim().TrimEnd([char]'\')
    foreach ($part in ($current -split ';')) {
        if ([string]::IsNullOrWhiteSpace($part)) { continue }
        if ($part.Trim().TrimEnd([char]'\').Equals($target, [StringComparison]::OrdinalIgnoreCase)) {
            return $false
        }
    }

    $updated = $current.TrimEnd([char]';') + ';' + $Directory
    [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
    return $true
}

$tempDir = $null

try {
    Assert-ShyenaEnvironment
    Write-Host 'Shyena installer: environment OK.'

    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    } catch {
        # TLS 1.2 is already the default on supported systems.
    }

    $tempDir = Join-Path ([IO.Path]::GetTempPath()) ('shyena-install-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

    $releaseBase = Get-ShyenaReleaseBase
    Write-Host "Downloading Shyena from $releaseBase ..."
    foreach ($asset in @('SHA256SUMS', 'shyena.exe', 'default.shyena')) {
        Save-ShyenaAsset -Name $asset -BaseUrl $releaseBase -Destination (Join-Path $tempDir $asset)
    }

    $checksums = Read-ShyenaChecksums -Path (Join-Path $tempDir 'SHA256SUMS')
    Test-ShyenaChecksums -Directory $tempDir -Expected $checksums
    Write-Host 'Checksums verified.'

    if ($DryRun) {
        Write-Host 'Dry run complete; no changes were made.'
        Write-Host "  Binary:    $exeTarget"
        Write-Host "  Config:    $configTarget"
        Write-Host "  Autostart: $runKey -> Shyena"
        return
    }

    $running = @(Get-Process -Name 'shyena' -ErrorAction SilentlyContinue)
    foreach ($process in $running) {
        Write-Host 'Stopping running Shyena instance...'
        try {
            Stop-Process -Id $process.Id -Force -ErrorAction Stop
            $process.WaitForExit(10000) | Out-Null
        } catch {
            # The process may have exited on its own.
        }
    }

    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $tempDir 'shyena.exe') -Destination $exeTarget -Force
    Write-Host "Installed binary: $exeTarget"

    if (Add-ShyenaToUserPath -Directory $installDir) {
        Write-Host "Added to user Path: $installDir (applies to new terminals)."
    } else {
        Write-Host "User Path already contains: $installDir"
    }

    if (-not (Test-Path -LiteralPath $configTarget)) {
        New-Item -ItemType Directory -Path $configDir -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $tempDir 'default.shyena') -Destination $configTarget
        Write-Host "Created config: $configTarget"
    } else {
        Write-Host "Kept existing config: $configTarget"
    }

    if (-not $NoAutostart) {
        if (-not (Test-Path -Path $runKey)) {
            New-Item -Path $runKey -Force | Out-Null
        }
        New-ItemProperty -Path $runKey -Name 'Shyena' -Value ('"' + $exeTarget + '"') -PropertyType String -Force | Out-Null
        Write-Host 'Autostart registered (HKCU Run -> Shyena).'
    } else {
        Write-Host 'Autostart skipped (-NoAutostart).'
    }

    if (-not $NoLaunch) {
        Start-Process -FilePath $exeTarget -WorkingDirectory $installDir
        Write-Host 'Shyena launched.'
    } else {
        Write-Host 'Launch skipped (-NoLaunch).'
    }

    Write-Host 'Shyena installation complete.' -ForegroundColor Green
} catch {
    Write-Host "Shyena installation failed: $($_.Exception.Message)" -ForegroundColor Red
} finally {
    if ($tempDir -and (Test-Path -LiteralPath $tempDir)) {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
