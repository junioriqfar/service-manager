[CmdletBinding()]
param(
    [string]$Root = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($Root)) {
    $Root = Split-Path -Parent $PSScriptRoot
}

$dataDir = Join-Path $Root "data"
$requestPath = Join-Path $dataDir "updater-request.json"
$resultPath = Join-Path $dataDir "updater-result.json"
$logDir = Join-Path $Root "logs"
$utf8 = New-Object System.Text.UTF8Encoding($false)

function Write-Result {
    param(
        [string]$Nonce,
        [int]$ExitCode,
        [string]$LogPath,
        [string]$ErrorText = ""
    )
    $payload = [ordered]@{
        nonce = $Nonce
        exitCode = $ExitCode
        logPath = $LogPath
        error = $ErrorText
        finishedAt = (Get-Date).ToUniversalTime().ToString("o")
    }
    $json = $payload | ConvertTo-Json -Compress
    $temp = "$resultPath.tmp"
    [System.IO.File]::WriteAllText($temp, $json, $utf8)
    Move-Item -LiteralPath $temp -Destination $resultPath -Force
}

if (-not (Test-Path -LiteralPath $requestPath)) {
    exit 1
}

$request = Get-Content -LiteralPath $requestPath -Raw | ConvertFrom-Json
$nonce = [string]$request.nonce
if ([string]::IsNullOrWhiteSpace($nonce)) {
    exit 1
}

if (-not (Test-Path -LiteralPath $logDir)) {
    New-Item -ItemType Directory -Path $logDir -Force | Out-Null
}
$logPath = Join-Path $logDir ("updater-task-" + $nonce + ".log")

try {
    $created = [datetime]::Parse([string]$request.createdAt).ToUniversalTime()
    $age = (Get-Date).ToUniversalTime() - $created
    if ($age.TotalMinutes -gt 15 -or $age.TotalMinutes -lt -1) {
        "Request trigger kedaluwarsa (umur $([math]::Round($age.TotalMinutes, 1)) menit)." |
            Set-Content -LiteralPath $logPath -Encoding UTF8
        Write-Result -Nonce $nonce -ExitCode 1 -LogPath $logPath -ErrorText "expired request"
        exit 1
    }
} catch {
    Write-Result -Nonce $nonce -ExitCode 1 -LogPath $logPath -ErrorText "invalid request timestamp"
    exit 1
}

$app = [string]$request.app
if ($app -and $app -notmatch "^[A-Za-z0-9._-]+$") {
    Write-Result -Nonce $nonce -ExitCode 1 -LogPath $logPath -ErrorText "invalid app id"
    exit 1
}

$clientIp = [string]$request.clientIp
if ($clientIp.StartsWith("::ffff:")) {
    $clientIp = $clientIp.Substring(7)
}
$parsedIp = $null
if ($clientIp -and -not [System.Net.IPAddress]::TryParse($clientIp, [ref]$parsedIp)) {
    $clientIp = ""
}

$updater = Join-Path $Root "sm-updater.exe"
if (-not (Test-Path -LiteralPath $updater)) {
    $updater = Join-Path $Root "target\release\sm-updater.exe"
}
if (-not (Test-Path -LiteralPath $updater)) {
    Write-Result -Nonce $nonce -ExitCode 1 -LogPath $logPath -ErrorText "sm-updater.exe not found"
    exit 1
}

$arguments = @("run")
if ($app) {
    $arguments += @("--app", $app)
} else {
    $arguments += "--all"
}
if ([bool]$request.check) {
    $arguments += "--check"
}
if (-not [string]::IsNullOrWhiteSpace([string]$request.config)) {
    $arguments += @("--config", [string]$request.config)
}
if ($clientIp) {
    $arguments += @("--client-ip", $clientIp)
}

try {
    & $updater @arguments *> $logPath
    $exitCode = $LASTEXITCODE
    Write-Result -Nonce $nonce -ExitCode $exitCode -LogPath $logPath
    exit $exitCode
} catch {
    "Task action gagal: $($_.Exception.Message)" | Add-Content -LiteralPath $logPath -Encoding UTF8
    Write-Result -Nonce $nonce -ExitCode 1 -LogPath $logPath -ErrorText $_.Exception.Message
    exit 1
}
