[CmdletBinding()]
param(
    [switch]$PurgeConfig,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$installDir = Join-Path $env:LOCALAPPDATA 'Programs\shyena'
$configDir = Join-Path $env:USERPROFILE '.config'
$configPath = Join-Path $configDir '.shyena'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

function Remove-ShyenaFromUserPath {
    param([string]$Directory)

    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ([string]::IsNullOrEmpty($current)) {
        return $false
    }

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

try {
    if (-not $installDir.EndsWith('Programs\shyena', [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove unexpected directory: $installDir"
    }

    if ($DryRun) {
        Write-Host 'Dry run complete; no changes were made.'
        Write-Host "  Process:   stop any running shyena.exe"
        Write-Host "  Autostart: $runKey -> Shyena"
        Write-Host "  Path:      remove $installDir from user Path"
        Write-Host "  Directory: $installDir"
        if ($PurgeConfig) {
            Write-Host "  Config:    remove $configPath"
        } else {
            Write-Host "  Config:    keep $configPath"
        }
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

    Remove-ItemProperty -Path $runKey -Name 'Shyena' -ErrorAction SilentlyContinue
    Write-Host 'Autostart entry removed (if present).'

    if (Remove-ShyenaFromUserPath -Directory $installDir) {
        Write-Host 'Removed install directory from user Path.'
    } else {
        Write-Host 'Install directory was not present in user Path.'
    }

    if (Test-Path -LiteralPath $installDir) {
        Remove-Item -LiteralPath $installDir -Recurse -Force
        Write-Host "Removed directory: $installDir"
    } else {
        Write-Host "Directory already absent: $installDir"
    }

    if ($PurgeConfig) {
        if (Test-Path -LiteralPath $configPath) {
            Remove-Item -LiteralPath $configPath -Force
            Write-Host "Removed config: $configPath"
        }
        if ((Test-Path -LiteralPath $configDir) -and @(Get-ChildItem -LiteralPath $configDir -Force).Count -eq 0) {
            Remove-Item -LiteralPath $configDir -Force
            Write-Host "Removed empty directory: $configDir"
        }
    } else {
        Write-Host "Kept config: $configPath"
    }

    Write-Host 'Shyena uninstall complete.' -ForegroundColor Green
} catch {
    Write-Host "Shyena uninstall failed: $($_.Exception.Message)" -ForegroundColor Red
}
