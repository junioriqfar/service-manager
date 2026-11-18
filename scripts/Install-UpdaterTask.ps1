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
    throw "Action script not found: $actionScript"
}
$hiddenLauncher = Join-Path $PSScriptRoot "Run-UpdaterTaskHidden.vbs"
if (-not (Test-Path -LiteralPath $hiddenLauncher)) {
    throw "Hidden launcher not found: $hiddenLauncher"
}
if ([string]::IsNullOrWhiteSpace($UserId)) {
    $UserId = "$env:USERDOMAIN\$env:USERNAME"
}

# wscript.exe is a GUI-subsystem host, so Task Scheduler never creates a console
# window. The VBScript starts PowerShell with window style 0 (hidden).
$action = New-ScheduledTaskAction `
    -Execute "wscript.exe" `
    -Argument "//B //Nologo `"$hiddenLauncher`"" `
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
        $Credential = Get-Credential -UserName $UserId -Message "Account password for Scheduled Task $TaskName"
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

Write-Host "Scheduled Task '$TaskName' registered."
Write-Host "User    : $UserId"
Write-Host "Logon   : $LogonType"
Write-Host "Action  : wscript.exe //B //Nologo `"$hiddenLauncher`" (PowerShell hidden)"
Write-Host ""
Write-Host "Test from an SSH session:"
Write-Host "  powershell -NoProfile -ExecutionPolicy Bypass -File `"$PSScriptRoot\Invoke-UpdaterTask.ps1`" -Check"
