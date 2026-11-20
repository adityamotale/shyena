[CmdletBinding()]
param(
    [string]$Version = 'latest',
    [string]$CurrentVersion = '',
    [string]$InstallDir = '',
    [switch]$NoLaunch,
    [switch]$DryRun,
    [switch]$Yes
)

$ErrorActionPreference = 'Stop'

$repoOwner = 'adityamotale'
$repoName = 'shyena'
$resolvedInstallDir = if (-not [string]::IsNullOrWhiteSpace($InstallDir)) {
    $InstallDir
} else {
    Join-Path $env:LOCALAPPDATA 'Programs\shyena'
}
$exeTarget = Join-Path $resolvedInstallDir 'shyena.exe'
$versionTarget = Join-Path $resolvedInstallDir 'version.txt'

$script:ShyenaAborted = $false
$script:ShyenaSpinnerTimer = $null
$script:ShyenaSpinnerSource = 'ShyenaUpdateSpinnerTick'
$script:ShyenaProgressTotal = 1
$script:ShyenaProgressDone = 0

# ---------------------------------------------------------------- visual helpers

function Write-ShyenaBanner {
    Write-Host ''
    Write-Host '  -------------------------------------------------------------' `
        -ForegroundColor DarkCyan
    Write-Host '   s h y e n a   -   updater' -ForegroundColor Cyan
    Write-Host "   target release: $Version" -ForegroundColor DarkGray
    Write-Host '   github.com/adityamotale/shyena' -ForegroundColor DarkGray
    Write-Host '  -------------------------------------------------------------' `
        -ForegroundColor DarkCyan
    Write-Host ''
}

function Write-ShyenaStep {
    param(
        [int]$Index,
        [int]$Total,
        [string]$Label,
        [ValidateSet('OK', 'SKIP', 'FAIL')][string]$Status = 'OK'
    )

    $color = switch ($Status) { 'OK' { 'Green' } 'SKIP' { 'Yellow' } 'FAIL' { 'Red' } }
    Write-Host -NoNewline ('  [{0}/{1}] {2} ' -f $Index, $Total, $Label.PadRight(42, '.'))
    Write-Host ('[{0}]' -f $Status) -ForegroundColor $color
}

function Write-ShyenaStatus {
    param(
        [string]$Label,
        [ValidateSet('OK', 'SKIP', 'FAIL')][string]$Status = 'OK'
    )

    $color = switch ($Status) { 'OK' { 'Green' } 'SKIP' { 'Yellow' } 'FAIL' { 'Red' } }
    Write-Host -NoNewline ('  [{0}] ' -f $Status) -ForegroundColor $color
    Write-Host $Label
}

function Start-ShyenaSpinner {
    param([string]$Message)

    $outRedirected = $false
    try { $outRedirected = [Console]::IsOutputRedirected } catch { }
    if (-not [Environment]::UserInteractive -or $outRedirected) { return }
    Stop-ShyenaSpinner

    $state = @{ Frames = @('|', '/', '-', '\'); Index = 0; Message = $Message }
    $timer = New-Object System.Timers.Timer(120)
    $timer.AutoReset = $true

    $script:ShyenaSpinnerTimer = $timer
    $null = Register-ObjectEvent -InputObject $timer -EventName Elapsed `
        -SourceIdentifier $script:ShyenaSpinnerSource -MessageData $state -Action {
            $s = $Event.MessageData
            $frame = $s.Frames[$s.Index % $s.Frames.Count]
            $s.Index++
            Write-Host -NoNewline ("`r  {0} {1}  " -f $frame, $s.Message)
        }
    $timer.Start()
}

function Stop-ShyenaSpinner {
    $timer = $script:ShyenaSpinnerTimer
    $hadTimer = $null -ne $timer

    if ($timer) {
        try { $timer.Stop() } catch { }
        $timer.Dispose()
        $script:ShyenaSpinnerTimer = $null
    }
    Unregister-Event -SourceIdentifier $script:ShyenaSpinnerSource -ErrorAction SilentlyContinue
    if ($hadTimer -and [Environment]::UserInteractive) {
        Write-Host -NoNewline ("`r" + (' ' * 78) + "`r")
    }
}

function Step-ShyenaProgress {
    param([string]$Status)

    $script:ShyenaProgressDone++
    $percent = [int](100 * $script:ShyenaProgressDone / $script:ShyenaProgressTotal)
    Write-Progress -Activity 'Updating Shyena' -Status $Status -PercentComplete $percent
}

function Show-ShyenaPlan {
    param([Parameter(Mandatory = $true)][object[]]$Items)

    Write-Host ''
    Write-Host '  ---------------------------- PLAN -----------------------------' `
        -ForegroundColor DarkCyan
    foreach ($item in $Items) {
        if ($item.Do) {
            Write-Host ('  +  {0}' -f $item.Text) -ForegroundColor Green
        } else {
            Write-Host ('  -  {0}' -f $item.Text) -ForegroundColor DarkGray
        }
    }
    Write-Host '  -------------------------------------------------------------' `
        -ForegroundColor DarkCyan
    Write-Host ''
}

function Read-ShyenaConsent {
    param(
        [Parameter(Mandatory = $true)][string]$Prompt,
        [string]$Help,
        [ValidateSet('Yes', 'No')][string]$Default = 'Yes',
        [switch]$Auto
    )

    $hint = if ($Default -eq 'Yes') { 'Y/n' } else { 'y/N' }
    Write-Host ''
    Write-Host ('  ?  {0}' -f $Prompt) -ForegroundColor Cyan
    if (-not [string]::IsNullOrWhiteSpace($Help)) {
        Write-Host ('  |  {0}' -f $Help) -ForegroundColor DarkGray
    }

    if ($Auto -or -not [Environment]::UserInteractive) {
        Write-Host ('  >  Auto-selected: {0}' -f $Default) -ForegroundColor DarkGray
        return $Default -eq 'Yes'
    }

    while ($true) {
        $raw = Read-Host ('     [{0}]' -f $hint)
        if ([string]::IsNullOrWhiteSpace($raw)) {
            return $Default -eq 'Yes'
        }
        $answer = $raw.Trim()
        if ($answer -match '^(y|yes)$') { return $true }
        if ($answer -match '^(n|no)$') { return $false }
        Write-Host '     Please answer y or n.' -ForegroundColor Yellow
    }
}

function Stop-ShyenaUpdate {
    param([string]$Reason)
    $script:ShyenaAborted = $true
    throw $Reason
}

# ---------------------------------------------------------------- environment / release helpers

function Assert-ShyenaEnvironment {
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw 'Shyena requires a 64-bit Windows installation.'
    }

    $build = [System.Environment]::OSVersion.Version.Build
    if ($build -le 0) {
        try {
            $build = [int](Get-CimInstance -ClassName Win32_OperatingSystem).BuildNumber
        } catch { }
    }
    if ($build -gt 0 -and $build -lt 19041) {
        throw "Shyena requires Windows 10 build 19041 or newer (detected build $build)."
    }

    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Shyena must be updated from a non-elevated shell (standard user token).'
    }

    if ($PSVersionTable.PSVersion.Major -lt 5) {
        throw 'Shyena requires Windows PowerShell 5.1 or newer.'
    }
}

function Get-ShyenaLocalVersion {
    param(
        [string]$PassedVersion,
        [string]$ExePath,
        [string]$VersionFilePath
    )

    if (-not [string]::IsNullOrWhiteSpace($PassedVersion)) {
        return $PassedVersion.Trim().TrimStart('v')
    }

    if (Test-Path -LiteralPath $VersionFilePath) {
        $fileContent = (Get-Content -LiteralPath $VersionFilePath -Raw `
            -ErrorAction SilentlyContinue)
        if (-not [string]::IsNullOrWhiteSpace($fileContent)) {
            return $fileContent.Trim().TrimStart('v')
        }
    }

    if (Test-Path -LiteralPath $ExePath) {
        $prodVer = (Get-Item -LiteralPath $ExePath).VersionInfo.ProductVersion
        if (-not [string]::IsNullOrWhiteSpace($prodVer)) {
            return $prodVer.Trim().TrimStart('v')
        }
    }

    return '0.0.0'
}

function Resolve-ShyenaRemoteRelease {
    param(
        [string]$TargetVersion,
        [string]$Owner,
        [string]$Repo
    )

    $apiUrl = if ([string]::IsNullOrWhiteSpace($TargetVersion) -or $TargetVersion -eq 'latest') {
        "https://api.github.com/repos/$Owner/$Repo/releases/latest"
    } else {
        "https://api.github.com/repos/$Owner/$Repo/releases/tags/$TargetVersion"
    }

    $headers = @{
        'User-Agent' = 'Shyena-Updater'
        'Accept'     = 'application/vnd.github.v3+json'
    }

    $request = @{
        Uri        = $apiUrl
        Headers    = $headers
        TimeoutSec = 30
    }
    if ($PSVersionTable.PSVersion.Major -lt 6) {
        $request['UseBasicParsing'] = $true
    }

    try {
        $resp = Invoke-RestMethod @request
        return $resp
    } catch {
        throw "Failed to fetch release info from GitHub: $($_.Exception.Message)"
    }
}

function Save-ShyenaAsset {
    param(
        [string]$Name,
        [string]$BaseUrl,
        [string]$Destination
    )

    $ProgressPreference = 'SilentlyContinue'

    $uri = "$BaseUrl/$Name"
    $request = @{
        Uri        = $uri
        OutFile    = $Destination
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

function Test-ShyenaBinaryChecksum {
    param(
        [string]$BinaryPath,
        [hashtable]$Checksums
    )

    if (-not $Checksums.ContainsKey('shyena.exe')) {
        throw "SHA256SUMS does not contain an entry for shyena.exe."
    }

    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $BinaryPath).Hash.ToLowerInvariant()
    $expected = $Checksums['shyena.exe']
    if ($actual -ne $expected) {
        throw "Checksum mismatch for shyena.exe (expected $expected, got $actual)."
    }
}

# ---------------------------------------------------------------- main

$exitCode = 0
$tempDir = $null

try {
    Write-ShyenaBanner

    Assert-ShyenaEnvironment
    Write-ShyenaStep -Index 1 -Total 4 -Label 'Checking environment' -Status OK

    if (-not (Test-Path -LiteralPath $exeTarget)) {
        throw "Shyena is not installed at $exeTarget; please run install.ps1 instead."
    }

    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    } catch { }

    $localVersion = Get-ShyenaLocalVersion -PassedVersion $CurrentVersion `
        -ExePath $exeTarget -VersionFilePath $versionTarget

    Start-ShyenaSpinner -Message "Resolving remote release ($Version)"
    try {
        $release = Resolve-ShyenaRemoteRelease -TargetVersion $Version `
            -Owner $repoOwner -Repo $repoName
    } finally {
        Stop-ShyenaSpinner
    }

    $remoteTag = $release.tag_name
    $remoteVersion = $remoteTag.TrimStart('v')
    $releaseBase = "https://github.com/$repoOwner/$repoName/releases/download/$remoteTag"

    Write-ShyenaStep -Index 2 -Total 4 `
        -Label "Comparing versions ($localVersion -> $remoteVersion)" -Status OK

    try {
        $localSemver = [System.Version]::new($localVersion)
        $remoteSemver = [System.Version]::new($remoteVersion)
        $isOlderOrEqual = $remoteSemver -le $localSemver
    } catch {
        $isOlderOrEqual = ($remoteVersion -eq $localVersion)
    }

    if ($isOlderOrEqual -and ($Version -eq 'latest' -or [string]::IsNullOrWhiteSpace($Version))) {
        Write-Host ''
        Write-ShyenaStatus -Status OK `
            -Label "Shyena is already up to date (current: v$localVersion)."
        Write-Host ''
        if ([Environment]::UserInteractive -and -not $Yes) {
            Start-Sleep -Seconds 2
        }
        $running = @(Get-Process -Name 'shyena' -ErrorAction SilentlyContinue)
        if ($running.Count -eq 0 -and (Test-Path -LiteralPath $exeTarget)) {
            Start-Process -FilePath $exeTarget
        }
        exit 0
    }

    $tempDir = Join-Path ([IO.Path]::GetTempPath()) `
        ('shyena-update-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

    Start-ShyenaSpinner -Message 'Downloading SHA256SUMS and shyena.exe'
    try {
        Save-ShyenaAsset -Name 'SHA256SUMS' -BaseUrl $releaseBase `
            -Destination (Join-Path $tempDir 'SHA256SUMS')
        Save-ShyenaAsset -Name 'shyena.exe' -BaseUrl $releaseBase `
            -Destination (Join-Path $tempDir 'shyena.exe')
    } finally {
        Stop-ShyenaSpinner
    }
    Write-ShyenaStep -Index 3 -Total 4 -Label 'Downloading release assets' -Status OK

    $checksums = Read-ShyenaChecksums -Path (Join-Path $tempDir 'SHA256SUMS')
    Test-ShyenaBinaryChecksum -BinaryPath (Join-Path $tempDir 'shyena.exe') -Checksums $checksums
    Write-ShyenaStep -Index 4 -Total 4 -Label 'Verifying SHA256 checksums' -Status OK

    $running = @(Get-Process -Name 'shyena' -ErrorAction SilentlyContinue)

    $plan = @()
    if ($running.Count -gt 0) {
        $plan += [pscustomobject]@{
            Do   = $true
            Text = "Close $($running.Count) running Shyena instance(s)"
        }
    }
    $plan += [pscustomobject]@{
        Do   = $true
        Text = "Update binary -> $exeTarget (v$remoteVersion)"
    }
    $launchText = if (-not $NoLaunch) { 'Relaunch Shyena after update' } else { 'Skip launch' }
    $plan += [pscustomobject]@{ Do = (-not $NoLaunch); Text = $launchText }

    Show-ShyenaPlan -Items $plan

    if ($DryRun) {
        Write-Host '  Dry run complete - no changes were made.' -ForegroundColor Yellow
    } else {
        $proceed = Read-ShyenaConsent -Prompt "Update Shyena to v$remoteVersion now?" -Auto:$Yes
        if (-not $proceed) {
            Stop-ShyenaUpdate 'update was declined.'
        }

        $script:ShyenaProgressTotal = [Math]::Max(1, @($plan | Where-Object { $_.Do }).Count)
        $script:ShyenaProgressDone = 0

        if ($running.Count -gt 0) {
            foreach ($proc in $running) {
                try {
                    Stop-Process -Id $proc.Id -Force -ErrorAction Stop
                    $proc.WaitForExit(10000) | Out-Null
                } catch { }
            }
            Write-ShyenaStatus 'Stopped running Shyena instance.'
            Step-ShyenaProgress -Status 'Stopping running instance'
        }

        # Ensure write handle is unlocked before moving
        Start-Sleep -Milliseconds 300

        Copy-Item -LiteralPath (Join-Path $tempDir 'shyena.exe') -Destination $exeTarget -Force
        Set-Content -LiteralPath $versionTarget -Value $remoteVersion -Force -NoNewline
        Write-ShyenaStatus "Installed updated binary: $exeTarget (v$remoteVersion)"
        Step-ShyenaProgress -Status 'Installing updated binary'

        if (-not $NoLaunch) {
            Start-Process -FilePath $exeTarget
            Write-ShyenaStatus 'Launched updated Shyena.'
            Step-ShyenaProgress -Status 'Launching Shyena'
        }

        Write-Progress -Activity 'Updating Shyena' -Completed
        Write-Host ''
        Write-Host "  +  Shyena update to v$remoteVersion complete." -ForegroundColor Green
    }
} catch {
    if ($script:ShyenaAborted) {
        Write-Host ''
        Write-Host ('  Update aborted: {0}' -f $_.Exception.Message) -ForegroundColor Yellow
        $exitCode = 2
    } else {
        Write-Host ''
        Write-Host ('  Shyena update failed: {0}' -f $_.Exception.Message) -ForegroundColor Red
        $exitCode = 1
    }
} finally {
    Stop-ShyenaSpinner
    if ($tempDir -and (Test-Path -LiteralPath $tempDir)) {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if ($exitCode -ne 0 -and [Environment]::UserInteractive -and -not $Yes) {
    Write-Host ''
    Read-Host '  Press Enter to exit...'
}

exit $exitCode
