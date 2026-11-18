[CmdletBinding()]
param(
    [string]$App = "",
    [switch]$Check,
    [string]$Config = "",
    [string]$TaskName = "ServiceManager-Updater",
    [int]$TimeoutSeconds = 3600,
    [string]$UpdaterPath = ""
)

$ErrorActionPreference = "Stop"

function Get-RepoRoot {
    if ($PSScriptRoot) {
        return (Split-Path -Parent $PSScriptRoot)
    }
    return (Get-Location).Path
}

$root = Get-RepoRoot
$dataDir = Join-Path $root "data"
$requestPath = Join-Path $dataDir "updater-request.json"
$resultPath = Join-Path $dataDir "updater-result.json"

if (-not (Test-Path -LiteralPath $dataDir)) {
    New-Item -ItemType Directory -Path $dataDir -Force | Out-Null
}

if ([string]::IsNullOrWhiteSpace($UpdaterPath)) {
    $candidate = Join-Path $root "sm-updater.exe"
    if (Test-Path -LiteralPath $candidate) {
        $UpdaterPath = $candidate
    } else {
        $UpdaterPath = Join-Path $root "target\release\sm-updater.exe"
    }
}
if (-not (Test-Path -LiteralPath $UpdaterPath)) {
    [Console]::Error.WriteLine("sm-updater.exe not found: $UpdaterPath")
    exit 1
}

# Validasi IP di sesi SSH (env SSH_CONNECTION/SSH_CLIENT masih tersedia).
$authorizeArgs = @("authorize")
if (-not [string]::IsNullOrWhiteSpace($App)) {
    $authorizeArgs += @("--app", $App)
}
if (-not [string]::IsNullOrWhiteSpace($Config)) {
    $authorizeArgs += @("--config", $Config)
}
& $UpdaterPath @authorizeArgs
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$clientIp = ""
if (-not [string]::IsNullOrWhiteSpace($env:SSH_CONNECTION)) {
    $clientIp = ($env:SSH_CONNECTION -split "\s+")[0]
} elseif (-not [string]::IsNullOrWhiteSpace($env:SSH_CLIENT)) {
    $clientIp = ($env:SSH_CLIENT -split "\s+")[0]
}

$nonce = [guid]::NewGuid().ToString("N")
$request = [ordered]@{
    nonce = $nonce
    createdAt = (Get-Date).ToUniversalTime().ToString("o")
    app = $App
    check = [bool]$Check
    config = $Config
    clientIp = $clientIp
}
$json = $request | ConvertTo-Json -Compress
$utf8 = New-Object System.Text.UTF8Encoding($false)
$tempRequest = "$requestPath.tmp"
[System.IO.File]::WriteAllText($tempRequest, $json, $utf8)
Move-Item -LiteralPath $tempRequest -Destination $requestPath -Force

$task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
if ($null -eq $task) {
    [Console]::Error.WriteLine("Scheduled Task '$TaskName' is not installed. Run scripts\Install-UpdaterTask.ps1 as Administrator.")
    exit 1
}

Start-ScheduledTask -TaskName $TaskName
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
while ((Get-Date) -lt $deadline) {
    if (Test-Path -LiteralPath $resultPath) {
        $result = $null
        try {
            $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
        } catch {
            $result = $null
        }
        if ($null -ne $result -and $result.nonce -eq $nonce) {
            if (-not [string]::IsNullOrWhiteSpace($result.logPath) -and (Test-Path -LiteralPath $result.logPath)) {
                Get-Content -LiteralPath $result.logPath
            }
            $code = 0
            if ($null -ne $result.exitCode) {
                $code = [int]$result.exitCode
            }
            exit $code
        }
    }
    Start-Sleep -Milliseconds 500
}

[Console]::Error.WriteLine("Timed out waiting for Scheduled Task '$TaskName' to finish.")
exit 1
