[CmdletBinding()]
param(
    [switch]$PurgeConfig,
    [switch]$DryRun,
    [switch]$Yes
)

$ErrorActionPreference = 'Stop'

$installDir = Join-Path $env:LOCALAPPDATA 'Programs\shyena'
$exeTarget = Join-Path $installDir 'shyena.exe'
$configDir = Join-Path $env:USERPROFILE '.config'
$configPath = Join-Path $configDir '.shyena'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

$script:ShyenaAborted = $false
$script:ShyenaProgressTotal = 1
$script:ShyenaProgressDone = 0

# ---------------------------------------------------------------- visual helpers

function Write-ShyenaBanner {
    Write-Host ''
    Write-Host '  -------------------------------------------------------------' -ForegroundColor DarkCyan
    Write-Host '   s h y e n a   -   uninstaller' -ForegroundColor Cyan
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

function Step-ShyenaProgress {
    param([string]$Status)

    $script:ShyenaProgressDone++
    $percent = [int](100 * $script:ShyenaProgressDone / $script:ShyenaProgressTotal)
    Write-Progress -Activity 'Uninstalling Shyena' -Status $Status -PercentComplete $percent
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
        throw 'Interactive input is unavailable. Run this uninstaller in a terminal, or pass -Yes to accept the defaults.'
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

function Stop-ShyenaUninstall {
    param([string]$Message)

    $script:ShyenaAborted = $true
    throw $Message
}

# ---------------------------------------------------------------- path helpers

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

function Remove-ShyenaFromUserPath {
    param([string]$Directory)

    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ([string]::IsNullOrEmpty($current)) { return $false }

    $target = $Directory.Trim().TrimEnd([char]'\')
    $kept = @()
    $removed = $false
    foreach ($part in ($current -split ';')) {
        if ([string]::IsNullOrWhiteSpace($part)) { continue }
        if ($part.Trim().TrimEnd([char]'\').Equals($target, [StringComparison]::OrdinalIgnoreCase)) {
            $removed = $true
        } else {
            $kept += $part
        }
    }

    if ($removed) {
        [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')
    }
    return $removed
}

# ---------------------------------------------------------------- main

$exitCode = 0

try {
    Write-ShyenaBanner

    if (-not $installDir.EndsWith('Programs\shyena', [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove unexpected directory: $installDir"
    }
    Write-ShyenaStep -Index 1 -Total 1 -Label 'Checking install paths' -Status OK

    # Only stop the copy installed by this script, never an unrelated process
    # that happens to share the name.
    $running = @(Get-Process -Name 'shyena' -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -and $_.Path.Equals($exeTarget, [StringComparison]::OrdinalIgnoreCase) }
        catch { $false }
    })
    $autostartPresent = $null -ne (Get-ItemProperty -Path $runKey -Name 'Shyena' -ErrorAction SilentlyContinue)
    $pathPresent = Test-ShyenaInUserPath -Directory $installDir
    $dirPresent = Test-Path -LiteralPath $installDir
    $configPresent = Test-Path -LiteralPath $configPath

    if ($running.Count -eq 0) { Write-ShyenaStatus -Status SKIP -Label 'No running Shyena instance.' }
    if (-not $autostartPresent) { Write-ShyenaStatus -Status SKIP -Label 'No autostart entry registered.' }
    if (-not $pathPresent) { Write-ShyenaStatus -Status SKIP -Label 'Install dir not present in user Path.' }
    if (-not $dirPresent) { Write-ShyenaStatus -Status SKIP -Label "Directory already absent: $installDir" }
    if (-not $configPresent) { Write-ShyenaStatus -Status SKIP -Label "Config already absent: $configPath" }

    # Collect consent for every mutating action up front (Astro-style).
    if ($running.Count -gt 0) {
        $closeRunning = Get-ShyenaAnswer -Prompt "Close $($running.Count) running Shyena instance(s)?" -DefaultYes $true
        if (-not $closeRunning) {
            Stop-ShyenaUninstall 'a running instance must be closed before uninstalling.'
        }
    }

    $removeAutostart = $false
    if ($autostartPresent) {
        $removeAutostart = Get-ShyenaAnswer -Prompt 'Remove the autostart entry (HKCU Run -> Shyena)?' -DefaultYes $true
    }

    $removePath = $false
    if ($pathPresent) {
        $removePath = Get-ShyenaAnswer -Prompt 'Remove the install directory from your user Path?' -DefaultYes $true
    }

    $removeDir = $false
    if ($dirPresent) {
        $removeDir = Get-ShyenaAnswer -Prompt "Delete the install directory $installDir?" -DefaultYes $true
    }

    if ($PurgeConfig) {
        $purgeConfig = $configPresent
    } elseif ($configPresent) {
        $purgeConfig = Get-ShyenaAnswer -Prompt "Purge your config $configPath?" -Help 'This deletes your settings. Use -PurgeConfig to force it.' -DefaultYes $false
    } else {
        $purgeConfig = $false
    }

    $plan = @()
    if ($running.Count -gt 0) {
        $plan += [pscustomobject]@{ Do = $closeRunning; Text = "Close $($running.Count) running instance(s)" }
    }
    $plan += [pscustomobject]@{ Do = $removeAutostart; Text = $(if ($autostartPresent) { 'Remove autostart entry' } else { 'No autostart entry to remove' }) }
    $plan += [pscustomobject]@{ Do = $removePath; Text = $(if ($pathPresent) { 'Remove install dir from user Path' } else { 'User Path already clean' }) }
    $plan += [pscustomobject]@{ Do = $removeDir; Text = $(if ($dirPresent) { "Delete directory: $installDir" } else { 'Directory already absent' }) }
    if ($PurgeConfig -and -not $configPresent) {
        $plan += [pscustomobject]@{ Do = $false; Text = 'Config already absent' }
    } elseif ($purgeConfig) {
        $plan += [pscustomobject]@{ Do = $true; Text = "Purge config: $configPath" }
    } else {
        $plan += [pscustomobject]@{ Do = $false; Text = "Keep config: $configPath" }
    }

    Show-ShyenaPlan -Items $plan

    if ($DryRun) {
        Write-Host '  Dry run complete - no changes were made.' -ForegroundColor Yellow
    } else {
        $proceed = Read-ShyenaConsent -Prompt 'Proceed with these changes?' -Auto:$Yes
        if (-not $proceed) {
            Stop-ShyenaUninstall 'you chose not to proceed.'
        }

        $script:ShyenaProgressTotal = [Math]::Max(1, @($plan | Where-Object { $_.Do }).Count)
        $script:ShyenaProgressDone = 0

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
            Step-ShyenaProgress -Status 'Stopping running instance'
        }

        if ($removeAutostart) {
            Remove-ItemProperty -Path $runKey -Name 'Shyena' -ErrorAction SilentlyContinue
            Write-ShyenaStatus 'Autostart entry removed (if present).'
            Step-ShyenaProgress -Status 'Removing autostart entry'
        }

        if ($removePath) {
            if (Remove-ShyenaFromUserPath -Directory $installDir) {
                Write-ShyenaStatus 'Removed install directory from user Path.'
            } else {
                Write-ShyenaStatus -Status SKIP -Label 'Install directory was not present in user Path.'
            }
            Step-ShyenaProgress -Status 'Updating user Path'
        }

        if ($removeDir) {
            if (Test-Path -LiteralPath $installDir) {
                Remove-Item -LiteralPath $installDir -Recurse -Force
                Write-ShyenaStatus "Removed directory: $installDir"
            } else {
                Write-ShyenaStatus -Status SKIP -Label "Directory already absent: $installDir"
            }
            Step-ShyenaProgress -Status 'Deleting install directory'
        }

        if ($purgeConfig) {
            if (Test-Path -LiteralPath $configPath) {
                Remove-Item -LiteralPath $configPath -Force
                Write-ShyenaStatus "Removed config: $configPath"
            }
            if ((Test-Path -LiteralPath $configDir) -and @(Get-ChildItem -LiteralPath $configDir -Force).Count -eq 0) {
                Remove-Item -LiteralPath $configDir -Force
                Write-ShyenaStatus "Removed empty directory: $configDir"
            }
            Step-ShyenaProgress -Status 'Purging config'
        } elseif ($configPresent) {
            Write-ShyenaStatus -Status SKIP -Label "Kept config: $configPath"
        }

        Write-Progress -Activity 'Uninstalling Shyena' -Completed
        Write-Host ''
        Write-Host '  +  Shyena uninstall complete.' -ForegroundColor Green
    }
}
catch {
    if ($script:ShyenaAborted) {
        Write-Host ''
        Write-Host ('  Uninstall aborted: {0}' -f $_.Exception.Message) -ForegroundColor Yellow
        $exitCode = 2
    } else {
        Write-Host ''
        Write-Host ('  Shyena uninstall failed: {0}' -f $_.Exception.Message) -ForegroundColor Red
        $exitCode = 1
    }
}

exit $exitCode
