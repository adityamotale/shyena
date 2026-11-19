[CmdletBinding()]
param(
    [string]$Version = 'latest',
    [switch]$NoLaunch,
    [switch]$NoAutostart,
    [switch]$DryRun,
    [switch]$Yes
)

$ErrorActionPreference = 'Stop'

$repoOwner = 'adityamotale'
$repoName = 'shyena'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\shyena'
$exeTarget = Join-Path $installDir 'shyena.exe'
$configDir = Join-Path $env:USERPROFILE '.config'
$configTarget = Join-Path $configDir '.shyena'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

$script:ShyenaAborted = $false
$script:ShyenaSpinnerTimer = $null
$script:ShyenaSpinnerSource = 'ShyenaSpinnerTick'
$script:ShyenaProgressTotal = 1
$script:ShyenaProgressDone = 0

# ---------------------------------------------------------------- visual helpers

function Write-ShyenaBanner {
    Write-Host ''
    Write-Host '  -------------------------------------------------------------' -ForegroundColor DarkCyan
    Write-Host '   s h y e n a   -   installer' -ForegroundColor Cyan
    Write-Host "   release: $Version" -ForegroundColor DarkGray
    Write-Host '   github.com/adityamotale/shyena' -ForegroundColor DarkGray
    Write-Host '  -------------------------------------------------------------' -ForegroundColor DarkCyan
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
    Write-Progress -Activity 'Installing Shyena' -Status $Status -PercentComplete $percent
}

function Show-ShyenaPlan {
    param([Parameter(Mandatory = $true)][object[]]$Items)

    Write-Host ''
    Write-Host '  ---------------------------- PLAN -----------------------------' -ForegroundColor DarkCyan
    foreach ($item in $Items) {
        if ($item.Do) {
            Write-Host ('  +  {0}' -f $item.Text) -ForegroundColor Green
        } else {
            Write-Host ('  -  {0}' -f $item.Text) -ForegroundColor DarkGray
        }
    }
    Write-Host '  -------------------------------------------------------------' -ForegroundColor DarkCyan
    Write-Host ''
}

# ---------------------------------------------------------------- consent helpers

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

    if ($Auto) {
        Write-Host ('  >  {0} : {1} (auto)' -f $hint, $Default) -ForegroundColor DarkGray
        return ($Default -eq 'Yes')
    }

    $redirected = $false
    try { $redirected = [Console]::IsInputRedirected } catch { }
    if (-not [Environment]::UserInteractive -or $redirected) {
        throw 'Interactive input is unavailable. Run this installer in a terminal, or pass -Yes to accept the defaults.'
    }

    while ($true) {
        Write-Host -NoNewline ('  >  {0} : ' -f $hint) -ForegroundColor Cyan
        $answer = (Read-Host).Trim().ToLowerInvariant()
        if ([string]::IsNullOrEmpty($answer)) {
            Write-Host ('     -> {0}' -f $Default) -ForegroundColor DarkGray
            return ($Default -eq 'Yes')
        }
        if ($answer -in @('y', 'yes')) {
            Write-Host '     -> Yes' -ForegroundColor Green
            return $true
        }
        if ($answer -in @('n', 'no')) {
            Write-Host '     -> No' -ForegroundColor Yellow
            return $false
        }
        Write-Host '     Please answer y or n.' -ForegroundColor Red
    }
}

function Get-ShyenaAnswer {
    param(
        [Parameter(Mandatory = $true)][string]$Prompt,
        [string]$Help,
        [bool]$DefaultYes = $true
    )

    if ($DryRun) { return $DefaultYes }
    $default = if ($DefaultYes) { 'Yes' } else { 'No' }
    return Read-ShyenaConsent -Prompt $Prompt -Help $Help -Default $default -Auto:$Yes
}

function Stop-ShyenaInstall {
    param([string]$Message)

    $script:ShyenaAborted = $true
    throw $Message
}

# ---------------------------------------------------------------- environment / release helpers

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

    # Keep the native download progress bar from fighting our animations.
    $ProgressPreference = 'SilentlyContinue'

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
        [hashtable]$Expected,
        [string]$ConfigName
    )

    foreach ($name in @('shyena.exe', $ConfigName)) {
        if (-not $Expected.ContainsKey($name)) {
            throw "SHA256SUMS does not contain an entry for $name."
        }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $Directory $name)).Hash.ToLowerInvariant()
        if ($actual -ne $Expected[$name]) {
            throw "Checksum mismatch for $name (expected $($Expected[$name]), got $actual)."
        }
    }
}

function Test-ShyenaInUserPath {
    param([string]$Directory)

    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ([string]::IsNullOrEmpty($current)) { return $false }

    $target = $Directory.Trim().TrimEnd([char]'\')
    foreach ($part in ($current -split ';')) {
        if ([string]::IsNullOrWhiteSpace($part)) { continue }
        if ($part.Trim().TrimEnd([char]'\').Equals($target, [StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }
    return $false
}

function Add-ShyenaToUserPath {
    param([string]$Directory)

    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ([string]::IsNullOrEmpty($current)) {
        [Environment]::SetEnvironmentVariable('Path', $Directory, 'User')
        return $true
    }

    if (Test-ShyenaInUserPath -Directory $Directory) { return $false }

    $updated = $current.TrimEnd([char]';') + ';' + $Directory
    [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
    return $true
}

# ---------------------------------------------------------------- main

$exitCode = 0
$tempDir = $null

try {
    Write-ShyenaBanner

    Assert-ShyenaEnvironment
    Write-ShyenaStep -Index 1 -Total 4 -Label 'Checking environment' -Status OK

    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    } catch {
        # TLS 1.2 is already the default on supported systems.
    }

    $releaseBase = Get-ShyenaReleaseBase
    Write-ShyenaStep -Index 2 -Total 4 -Label "Resolving release ($Version)" -Status OK

    $tempDir = Join-Path ([IO.Path]::GetTempPath()) ('shyena-install-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

    Start-ShyenaSpinner -Message 'Downloading SHA256SUMS'
    try {
        Save-ShyenaAsset -Name 'SHA256SUMS' -BaseUrl $releaseBase -Destination (Join-Path $tempDir 'SHA256SUMS')
    } finally {
        Stop-ShyenaSpinner
    }

    $checksums = Read-ShyenaChecksums -Path (Join-Path $tempDir 'SHA256SUMS')

    # New releases ship the config as '.shyena'; older tags used 'default.shyena'.
    if ($checksums.ContainsKey('.shyena')) {
        $configAsset = '.shyena'
    } elseif ($checksums.ContainsKey('default.shyena')) {
        $configAsset = 'default.shyena'
    } else {
        throw 'SHA256SUMS does not contain an entry for the config asset.'
    }

    Start-ShyenaSpinner -Message 'Downloading shyena.exe and default config'
    try {
        foreach ($asset in @('shyena.exe', $configAsset)) {
            Save-ShyenaAsset -Name $asset -BaseUrl $releaseBase -Destination (Join-Path $tempDir $asset)
        }
    } finally {
        Stop-ShyenaSpinner
    }
    Write-ShyenaStep -Index 3 -Total 4 -Label 'Downloading release assets' -Status OK

    Test-ShyenaChecksums -Directory $tempDir -Expected $checksums -ConfigName $configAsset
    Write-ShyenaStep -Index 4 -Total 4 -Label 'Verifying SHA256 checksums' -Status OK

    # Detect current state so prompts can be skipped when they would be no-ops.
    $running = @(Get-Process -Name 'shyena' -ErrorAction SilentlyContinue)
    $pathPresent = Test-ShyenaInUserPath -Directory $installDir
    $configExists = Test-Path -LiteralPath $configTarget

    if ($pathPresent) {
        Write-ShyenaStatus -Status SKIP -Label "User Path already contains: $installDir"
    }
    if ($configExists) {
        Write-ShyenaStatus -Status SKIP -Label "Config already exists - left untouched: $configTarget"
    }

    # Collect consent for every mutating action up front (Astro-style).
    $installBinary = Get-ShyenaAnswer -Prompt "Install the binary to $exeTarget?" -Help 'Replaces any existing shyena.exe.' -DefaultYes $true
    if (-not $installBinary) {
        Stop-ShyenaInstall 'binary installation was declined.'
    }

    $closeRunning = $true
    if ($running.Count -gt 0) {
        $closeRunning = Get-ShyenaAnswer -Prompt "Close $($running.Count) running Shyena instance(s)?" -Help 'Required before replacing shyena.exe.' -DefaultYes $true
        if (-not $closeRunning) {
            Stop-ShyenaInstall 'a running instance must be closed to replace the binary.'
        }
    }

    if ($pathPresent) {
        $addPath = $false
    } else {
        $addPath = Get-ShyenaAnswer -Prompt 'Add the install folder to your user Path?' -Help 'New terminals will see shyena on PATH.' -DefaultYes $true
    }

    if ($configExists) {
        $writeConfig = $false
    } else {
        $writeConfig = Get-ShyenaAnswer -Prompt "Write the default config to $configTarget?" -Help 'Seeded once from the release default; never overwritten later.' -DefaultYes $true
        if (-not $writeConfig) {
            Stop-ShyenaInstall 'config creation was declined; Shyena needs a config file.'
        }
    }

    if ($NoAutostart) {
        $autostart = $false
    } else {
        $autostart = Get-ShyenaAnswer -Prompt 'Register Shyena to start at logon?' -Help 'Adds HKCU Run -> Shyena.' -DefaultYes $true
    }

    if ($NoLaunch) {
        $launch = $false
    } else {
        $launch = Get-ShyenaAnswer -Prompt 'Launch Shyena when finished?' -DefaultYes $true
    }

    $plan = @()
    $plan += [pscustomobject]@{ Do = $installBinary; Text = "Install binary -> $exeTarget" }
    if ($running.Count -gt 0) {
        $plan += [pscustomobject]@{ Do = $closeRunning; Text = "Close $($running.Count) running instance(s)" }
    }
    if ($pathPresent) {
        $plan += [pscustomobject]@{ Do = $false; Text = 'Keep user Path (already contains install dir)' }
    } elseif ($addPath) {
        $plan += [pscustomobject]@{ Do = $true; Text = 'Add install dir to user Path' }
    } else {
        $plan += [pscustomobject]@{ Do = $false; Text = 'Skip adding install dir to user Path (declined)' }
    }
    if ($configExists) {
        $plan += [pscustomobject]@{ Do = $false; Text = "Keep existing config -> $configTarget" }
    } else {
        $plan += [pscustomobject]@{ Do = $true; Text = "Write default config -> $configTarget" }
    }
    if ($NoAutostart) {
        $plan += [pscustomobject]@{ Do = $false; Text = 'Skip autostart (-NoAutostart)' }
    } else {
        $plan += [pscustomobject]@{ Do = $autostart; Text = $(if ($autostart) { 'Register autostart (HKCU Run -> Shyena)' } else { 'Skip autostart (declined)' }) }
    }
    if ($NoLaunch) {
        $plan += [pscustomobject]@{ Do = $false; Text = 'Skip launch (-NoLaunch)' }
    } else {
        $plan += [pscustomobject]@{ Do = $launch; Text = $(if ($launch) { 'Launch Shyena on completion' } else { 'Skip launch (declined)' }) }
    }

    Show-ShyenaPlan -Items $plan

    if ($DryRun) {
        Write-Host '  Dry run complete - no changes were made.' -ForegroundColor Yellow
    } else {
        $proceed = Read-ShyenaConsent -Prompt 'Proceed with these changes?' -Auto:$Yes
        if (-not $proceed) {
            Stop-ShyenaInstall 'you chose not to proceed.'
        }

        $script:ShyenaProgressTotal = [Math]::Max(1, @($plan | Where-Object { $_.Do }).Count)
        $script:ShyenaProgressDone = 0

        if ($installBinary) {
            if ($running.Count -gt 0) {
                foreach ($process in $running) {
                    try {
                        Stop-Process -Id $process.Id -Force -ErrorAction Stop
                        $process.WaitForExit(10000) | Out-Null
                    } catch {
                        # The process may have exited on its own.
                    }
                }
                Write-ShyenaStatus 'Stopped running Shyena instance.'
            }
            New-Item -ItemType Directory -Path $installDir -Force | Out-Null
            Copy-Item -LiteralPath (Join-Path $tempDir 'shyena.exe') -Destination $exeTarget -Force
            Write-ShyenaStatus "Installed binary: $exeTarget"
            Step-ShyenaProgress -Status 'Installing binary'
        }

        if ($addPath) {
            if (Add-ShyenaToUserPath -Directory $installDir) {
                Write-ShyenaStatus "Added to user Path: $installDir"
            } else {
                Write-ShyenaStatus -Status SKIP -Label "User Path already contains: $installDir"
            }
            Step-ShyenaProgress -Status 'Updating user Path'
        } elseif ($pathPresent) {
            Write-ShyenaStatus -Status SKIP -Label "User Path already contains: $installDir"
        }

        if ($writeConfig) {
            New-Item -ItemType Directory -Path $configDir -Force | Out-Null
            Copy-Item -LiteralPath (Join-Path $tempDir $configAsset) -Destination $configTarget
            Write-ShyenaStatus "Created config: $configTarget"
            Step-ShyenaProgress -Status 'Writing config'
        } elseif ($configExists) {
            Write-ShyenaStatus -Status SKIP -Label "Config already exists - left untouched: $configTarget"
        }

        if ($autostart) {
            if (-not (Test-Path -Path $runKey)) {
                New-Item -Path $runKey -Force | Out-Null
            }
            New-ItemProperty -Path $runKey -Name 'Shyena' -Value ('"' + $exeTarget + '"') -PropertyType String -Force | Out-Null
            Write-ShyenaStatus 'Autostart registered (HKCU Run -> Shyena).'
            Step-ShyenaProgress -Status 'Registering autostart'
        }

        if ($launch) {
            Start-Process -FilePath $exeTarget -WorkingDirectory $installDir
            Write-ShyenaStatus 'Shyena launched.'
            Step-ShyenaProgress -Status 'Launching Shyena'
        }

        Write-Progress -Activity 'Installing Shyena' -Completed
        Write-Host ''
        Write-Host '  +  Shyena installation complete.' -ForegroundColor Green
        if ($addPath) {
            Write-Host '     New terminals will see shyena on PATH.' -ForegroundColor DarkGray
        }
    }
}
catch {
    if ($script:ShyenaAborted) {
        Write-Host ''
        Write-Host ('  Installation aborted: {0}' -f $_.Exception.Message) -ForegroundColor Yellow
        $exitCode = 2
    } else {
        Write-Host ''
        Write-Host ('  Shyena installation failed: {0}' -f $_.Exception.Message) -ForegroundColor Red
        $exitCode = 1
    }
}
finally {
    Stop-ShyenaSpinner
    if ($tempDir -and (Test-Path -LiteralPath $tempDir)) {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$global:LASTEXITCODE = $exitCode

# In a real terminal, wait for Enter so a dedicated console window or tab stays
# open; redirected/non-interactive runs keep the process exit code for automation.
$redirected = $false
try { $redirected = [Console]::IsInputRedirected } catch { }
if (-not [Environment]::UserInteractive -or $redirected) {
    exit $exitCode
}

Write-Host ''
Read-Host 'Press Enter to finish'
