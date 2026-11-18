#requires -RunAsAdministrator
[CmdletBinding()]
param(
    [string]$TaskName = "ServiceManager-Updater",
    [string]$UserId = "",
    [ValidateSet("Interactive", "Password", "S4U")]
    [string]$LogonType = "Interactive",
    [System.Management.Automation.PSCredential]$Credential
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$actionScript = Join-Path $PSScriptRoot "Run-UpdaterTaskAction.ps1"
if (-not (Test-Path -LiteralPath $actionScript)) {
    throw "Action script tidak ditemukan: $actionScript"
}
if ([string]::IsNullOrWhiteSpace($UserId)) {
    $UserId = "$env:USERDOMAIN\$env:USERNAME"
}

$action = New-ScheduledTaskAction `
    -Execute "powershell.exe" `
    -Argument "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$actionScript`"" `
    -WorkingDirectory $root

$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -MultipleInstances IgnoreNew `
    -Hidden `
    -ExecutionTimeLimit (New-TimeSpan -Hours 2)

$principalParams = @{
    UserId = $UserId
    LogonType = $LogonType
    RunLevel = "Highest"
}
if ($LogonType -eq "Password") {
    if ($null -eq $Credential) {
        $Credential = Get-Credential -UserName $UserId -Message "Password akun untuk Scheduled Task $TaskName"
    }
    $principalParams.UserId = $Credential.UserName
    $principalParams.Password = $Credential.GetNetworkCredential().Password
}
$principal = New-ScheduledTaskPrincipal @principalParams

Register-ScheduledTask `
    -TaskName $TaskName `
    -Action $action `
    -Settings $settings `
    -Principal $principal `
    -Description "Manual elevated trigger for Service Manager updater" `
    -Force | Out-Null

Write-Host "Scheduled Task '$TaskName' terdaftar."
Write-Host "User    : $UserId"
Write-Host "Logon   : $LogonType"
Write-Host "Action  : powershell.exe -File `"$actionScript`""
Write-Host ""
Write-Host "Uji dari sesi SSH:"
Write-Host "  powershell -NoProfile -ExecutionPolicy Bypass -File `"$PSScriptRoot\Invoke-UpdaterTask.ps1`" -Check"
