param(
    [switch]$Diagnose
)

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

[System.Windows.Forms.Application]::EnableVisualStyles()
[System.Windows.Forms.Application]::SetCompatibleTextRenderingDefault($false)

if (-not ('ServiceDelayHelper' -as [type])) {
    $serviceDelayCode = @'
using System;
using System.Runtime.InteropServices;

public static class ServiceDelayHelper
{
    [StructLayout(LayoutKind.Sequential)]
    public struct SERVICE_DELAYED_AUTO_START_INFO
    {
        public int fDelayedAutostart;
    }

    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern IntPtr OpenSCManagerW(string machineName, string databaseName, uint access);

    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern IntPtr OpenServiceW(IntPtr scManager, string serviceName, uint access);

    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern bool ChangeServiceConfig2(IntPtr service, uint infoLevel, ref SERVICE_DELAYED_AUTO_START_INFO info);

    [DllImport("advapi32.dll")]
    private static extern bool CloseServiceHandle(IntPtr handle);

    public static void SetDelayed(string serviceName, bool enabled)
    {
        IntPtr scm = OpenSCManagerW(null, null, 1);
        if (scm == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());

        try
        {
            IntPtr service = OpenServiceW(scm, serviceName, 2);
            if (service == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());

            try
            {
                SERVICE_DELAYED_AUTO_START_INFO info = new SERVICE_DELAYED_AUTO_START_INFO();
                info.fDelayedAutostart = enabled ? 1 : 0;
                if (!ChangeServiceConfig2(service, 3, ref info)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            }
            finally
            {
                CloseServiceHandle(service);
            }
        }
        finally
        {
            CloseServiceHandle(scm);
        }
    }
}
'@
    try {
        Add-Type -TypeDefinition $serviceDelayCode -ErrorAction Stop
    } catch {
        Write-Output "Peringatan: helper delayed start tidak bisa dimuat: $($_.Exception.Message)"
    }
}

$script:AppDir = $PSScriptRoot
$script:ConfigPath = Join-Path $script:AppDir 'services.json'
$script:BackupDir = Join-Path $script:AppDir 'backup'
$script:LogDir = Join-Path $script:AppDir 'logs'
$script:AppLogPath = Join-Path $script:LogDir 'app.log'
$script:NssmPath = Join-Path (Split-Path -Parent $script:AppDir) 'nssm\nssm.exe'

if (-not (Test-Path -LiteralPath $script:NssmPath)) {
    $fallbackNssm = 'C:\tools\nssm\nssm.exe'
    if (Test-Path -LiteralPath $fallbackNssm) { $script:NssmPath = $fallbackNssm }
}

foreach ($dir in @($script:AppDir, $script:BackupDir, $script:LogDir)) {
    if (-not (Test-Path -LiteralPath $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
}

$script:Form = $null
$script:Split = $null
$script:Grid = $null
$script:LogBox = $null
$script:LogLabel = $null
$script:AutoRefreshLog = $null
$script:StatusLabel = $null
$script:CountLabel = $null
$script:StatusFont = $null
$script:StatusTimer = $null
$script:LogTimer = $null
$script:SuppressEvents = $false
$script:LastLogText = ''
$script:Services = $null

$script:DlgForm = $null
$script:DlgType = $null
$script:DlgName = $null
$script:DlgDisplay = $null
$script:DlgExe = $null
$script:DlgLblExe = $null
$script:DlgLblArgs = $null
$script:DlgLblWork = $null
$script:DlgLblApacheRoot = $null
$script:DlgLblApacheConfig = $null
$script:DlgLblPgData = $null
$script:DlgLblPgAccount = $null
$script:DlgApacheRoot = $null
$script:DlgApacheConfig = $null
$script:DlgPgData = $null
$script:DlgPgAccount = $null
$script:DlgApacheRootBrowse = $null
$script:DlgApacheConfigBrowse = $null
$script:DlgPgDataBrowse = $null
$script:DlgHint = $null
$script:DlgArgs = $null
$script:DlgWork = $null
$script:DlgStdout = $null
$script:DlgStderr = $null
$script:DlgSame = $null
$script:DlgStartup = $null
$script:DlgRestart = $null
$script:DlgDelay = $null
$script:DlgDeps = $null
$script:DlgStart = $null
$script:DlgOutBrowse = $null
$script:DlgErrBrowse = $null
$script:DlgWorkBrowse = $null
$script:DlgService = $null
$script:DlgIsNew = $false

$script:ImportForm = $null
$script:ImportList = $null

function Test-Admin {
    try {
        $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
        $principal = New-Object System.Security.Principal.WindowsPrincipal($identity)
        return $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)
    } catch {
        return $false
    }
}

function Write-AppLog {
    param([string]$Message)
    try {
        $line = '{0}  {1}' -f (Get-Date).ToString('yyyy-MM-dd HH:mm:ss'), $Message
        Add-Content -LiteralPath $script:AppLogPath -Value $line -Encoding UTF8
    } catch {
    }
}

function New-UiPoint {
    param([int]$X, [int]$Y)
    return (New-Object System.Drawing.Point -ArgumentList $X, $Y)
}

function New-UiSize {
    param([int]$Width, [int]$Height)
    return (New-Object System.Drawing.Size -ArgumentList $Width, $Height)
}

function New-RowStyle {
    param([System.Windows.Forms.SizeType]$SizeType, [float]$Height)
    $style = New-Object System.Windows.Forms.RowStyle
    $style.SizeType = $SizeType
    $style.Height = $Height
    return $style
}

function New-ColumnStyle {
    param([System.Windows.Forms.SizeType]$SizeType, [float]$Width)
    $style = New-Object System.Windows.Forms.ColumnStyle
    $style.SizeType = $SizeType
    $style.Width = $Width
    return $style
}

function Show-Info {
    param([string]$Message)
    if ($null -ne $script:Form) {
        [System.Windows.Forms.MessageBox]::Show($script:Form, $Message, 'Service Manager', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Information) | Out-Null
    } else {
        [System.Windows.Forms.MessageBox]::Show($Message, 'Service Manager', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Information) | Out-Null
    }
}

function Show-Error {
    param([string]$Message)
    if ($null -ne $script:Form) {
        [System.Windows.Forms.MessageBox]::Show($script:Form, $Message, 'Service Manager', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Error) | Out-Null
    } else {
        [System.Windows.Forms.MessageBox]::Show($Message, 'Service Manager', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Error) | Out-Null
    }
}

function Set-AppStatus {
    param([string]$Text)
    if ($null -ne $script:StatusLabel) {
        $script:StatusLabel.Text = '{0}  {1}' -f (Get-Date).ToString('HH:mm:ss'), $Text
    }
}

function Invoke-UiAction {
    param([scriptblock]$Action)
    if ($null -ne $script:Form) {
        $script:Form.UseWaitCursor = $true
    }
    try {
        & $Action
    } catch {
        Write-AppLog "ERROR: $($_.Exception.Message)"
        Show-Error $_.Exception.Message
    } finally {
        if ($null -ne $script:Form) {
            $script:Form.UseWaitCursor = $false
        }
    }
}

function Get-StringProperty {
    param($Object, [string]$Name, [string]$Default = '')
    if ($null -eq $Object) { return $Default }
    $prop = $Object.PSObject.Properties[$Name]
    if ($null -eq $prop -or $null -eq $prop.Value) { return $Default }
    return [string]$prop.Value
}

function Split-Dependencies {
    param([string]$Text)
    if ([string]::IsNullOrWhiteSpace($Text)) { return @() }
    return @($Text -split '[,;\s]+' | Where-Object { $_ })
}

function Get-ServiceTypeLabel {
    param([string]$Type)
    switch ($Type) {
        'apache' { return 'Apache' }
        'postgresql' { return 'PostgreSQL' }
        default { return 'NSSM' }
    }
}

function Test-NativeServiceType {
    param([string]$Type)
    return ($Type -eq 'apache' -or $Type -eq 'postgresql')
}

function Get-ServiceConfigByName {
    param([string]$Name)
    if ($null -eq $script:Services) { return $null }
    foreach ($svc in $script:Services) {
        if ([string]::Equals([string]$svc.name, $Name, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $svc
        }
    }
    return $null
}

function Get-ManagedServiceType {
    param([string]$Name)
    $svc = Get-ServiceConfigByName $Name
    if ($null -ne $svc -and $null -ne $svc.PSObject.Properties['serviceType']) {
        $type = [string]$svc.serviceType
        if (-not [string]::IsNullOrWhiteSpace($type)) { return $type }
    }
    return 'nssm'
}

function Get-ExecutableFromPathName {
    param([string]$PathName)
    if ([string]::IsNullOrWhiteSpace($PathName)) { return '' }
    $text = $PathName.Trim()
    if ($text.StartsWith('"')) {
        $end = $text.IndexOf('"', 1)
        if ($end -gt 1) { return $text.Substring(1, $end - 1) }
    }
    $index = $text.IndexOf('.exe', [System.StringComparison]::OrdinalIgnoreCase)
    if ($index -ge 0) {
        return $text.Substring(0, $index + 4).Trim()
    }
    return ($text -split '\s+')[0].Trim()
}

function Get-CommandLineArgument {
    param([string]$CommandLine, [string]$Name)
    if ([string]::IsNullOrWhiteSpace($CommandLine)) { return '' }
    $escaped = [regex]::Escape($Name)
    $match = [regex]::Match($CommandLine, ('(?i)(?:^|\s)' + $escaped + '\s+"([^"]*)"'))
    if ($match.Success) { return $match.Groups[1].Value }
    $match = [regex]::Match($CommandLine, ('(?i)(?:^|\s)' + $escaped + '\s+([^\s"]+)'))
    if ($match.Success) { return $match.Groups[1].Value }
    return ''
}

function Get-ApacheRootFromExe {
    param([string]$Exe)
    if ([string]::IsNullOrWhiteSpace($Exe)) { return '' }
    $bin = Split-Path -Parent $Exe
    if ([string]::Equals((Split-Path -Leaf $bin), 'bin', [System.StringComparison]::OrdinalIgnoreCase)) {
        return Split-Path -Parent $bin
    }
    return $bin
}

function Get-NewestLogFile {
    param([string]$Directory)
    if ([string]::IsNullOrWhiteSpace($Directory) -or -not (Test-Path -LiteralPath $Directory -PathType Container)) {
        return ''
    }
    $file = Get-ChildItem -LiteralPath $Directory -Filter '*.log' -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($null -ne $file) { return $file.FullName }
    return ''
}

function Invoke-ExternalCommand {
    param(
        [string]$FilePath,
        [string[]]$Arguments = @(),
        [switch]$IgnoreExitCode
    )
    if (-not (Test-Path -LiteralPath $FilePath -PathType Leaf)) {
        throw "File tidak ditemukan: $FilePath"
    }
    $output = & $FilePath @Arguments 2>&1
    $code = $LASTEXITCODE
    $text = (($output | ForEach-Object { $_.ToString() }) -join "`r`n").Trim()
    if ($code -ne 0 -and -not $IgnoreExitCode) {
        throw "Perintah gagal (exit $code): $FilePath $($Arguments -join ' ')`r`n$text"
    }
    return [pscustomobject]@{
        ExitCode = $code
        Output = $text
    }
}

function Invoke-ScCommand {
    param([string[]]$Arguments, [switch]$IgnoreExitCode)
    $sc = Join-Path $env:SystemRoot 'System32\sc.exe'
    return Invoke-ExternalCommand -FilePath $sc -Arguments $Arguments -IgnoreExitCode:$IgnoreExitCode
}

function Assert-NativeServiceConfig {
    param($Svc, [switch]$ForRemove)
    $type = [string]$Svc.serviceType
    if (-not (Test-NativeServiceType $type)) { throw "Service '$($Svc.name)' bukan service native." }

    if ([string]::IsNullOrWhiteSpace([string]$Svc.executable) -or -not (Test-Path -LiteralPath $Svc.executable -PathType Leaf)) {
        throw "Installer service '$($Svc.name)' tidak ditemukan: $($Svc.executable)"
    }

    $installerName = [System.IO.Path]::GetFileName([string]$Svc.executable)
    if ($type -eq 'apache' -and $installerName -ine 'httpd.exe') {
        throw "Installer Apache harus httpd.exe: $($Svc.executable)"
    }
    if ($type -eq 'postgresql' -and $installerName -ine 'pg_ctl.exe') {
        throw "Installer PostgreSQL harus pg_ctl.exe: $($Svc.executable)"
    }

    if ($type -eq 'apache') {
        if ([string]::IsNullOrWhiteSpace([string]$Svc.apacheRoot) -or -not (Test-Path -LiteralPath $Svc.apacheRoot -PathType Container)) {
            throw "Apache root tidak ditemukan: $($Svc.apacheRoot)"
        }
        $configPath = [string]$Svc.apacheConfig
        if ([string]::IsNullOrWhiteSpace($configPath)) { $configPath = 'conf/httpd.conf' }
        if (-not [System.IO.Path]::IsPathRooted($configPath)) {
            $configPath = Join-Path ([string]$Svc.apacheRoot) $configPath
        }
        if (-not (Test-Path -LiteralPath $configPath -PathType Leaf)) {
            throw "Config Apache tidak ditemukan: $configPath"
        }
    } elseif ($type -eq 'postgresql' -and -not $ForRemove) {
        if ([string]::IsNullOrWhiteSpace([string]$Svc.pgData) -or -not (Test-Path -LiteralPath $Svc.pgData -PathType Container)) {
            throw "PGDATA tidak ditemukan: $($Svc.pgData)"
        }
        if ([string]::IsNullOrWhiteSpace([string]$Svc.pgAccount)) {
            throw 'Service account PostgreSQL wajib diisi.'
        }
    }
}

function Get-NativeInstallPlan {
    param($Svc)
    $type = [string]$Svc.serviceType
    if ($type -eq 'apache') {
        $config = [string]$Svc.apacheConfig
        if ([string]::IsNullOrWhiteSpace($config)) { $config = 'conf/httpd.conf' }
        return [pscustomobject]@{
            FilePath = [string]$Svc.executable
            Arguments = @('-d', [string]$Svc.apacheRoot, '-f', $config, '-k', 'install', '-n', [string]$Svc.name)
            RemoveArguments = @('-d', [string]$Svc.apacheRoot, '-f', $config, '-k', 'uninstall', '-n', [string]$Svc.name)
        }
    }
    if ($type -eq 'postgresql') {
        return [pscustomobject]@{
            FilePath = [string]$Svc.executable
            Arguments = @('register', '-N', [string]$Svc.name, '-D', [string]$Svc.pgData, '-U', [string]$Svc.pgAccount, '-S', 'auto', '-w')
            RemoveArguments = @('unregister', '-N', [string]$Svc.name)
        }
    }
    throw "Jenis service '$type' tidak memiliki installer native."
}

function Get-ManagedServicesInStartOrder {
    $installed = New-Object System.Collections.ArrayList
    foreach ($svc in $script:Services) {
        if (Test-ServiceExists $svc.name) { [void]$installed.Add($svc) }
    }
    if ($installed.Count -eq 0) { return @() }

    $byName = @{}
    $indegree = @{}
    $dependents = @{}
    foreach ($svc in $installed) {
        $key = ([string]$svc.name).ToLowerInvariant()
        $byName[$key] = $svc
        $indegree[$key] = 0
        $dependents[$key] = New-Object System.Collections.ArrayList
    }

    foreach ($svc in $installed) {
        $key = ([string]$svc.name).ToLowerInvariant()
        foreach ($dep in @($svc.dependencies)) {
            $depKey = ([string]$dep).ToLowerInvariant()
            if ($byName.ContainsKey($depKey) -and $depKey -ne $key) {
                $indegree[$key] = $indegree[$key] + 1
                [void]$dependents[$depKey].Add($key)
            }
        }
    }

    $ordered = New-Object System.Collections.ArrayList
    $queue = New-Object System.Collections.ArrayList
    foreach ($svc in $installed) {
        $key = ([string]$svc.name).ToLowerInvariant()
        if ($indegree[$key] -eq 0) { [void]$queue.Add($key) }
    }

    while ($queue.Count -gt 0) {
        $key = [string]$queue[0]
        $queue.RemoveAt(0)
        [void]$ordered.Add($byName[$key])
        foreach ($dependent in @($dependents[$key])) {
            $indegree[$dependent] = $indegree[$dependent] - 1
            if ($indegree[$dependent] -eq 0) { [void]$queue.Add($dependent) }
        }
    }

    if ($ordered.Count -lt $installed.Count) {
        $added = @{}
        foreach ($svc in $ordered) { $added[([string]$svc.name).ToLowerInvariant()] = $true }
        foreach ($svc in $installed) {
            $key = ([string]$svc.name).ToLowerInvariant()
            if (-not $added.ContainsKey($key)) { [void]$ordered.Add($svc) }
        }
    }

    return $ordered.ToArray()
}

function Get-ManagedServicesInStopOrder {
    $order = @(Get-ManagedServicesInStartOrder)
    [array]::Reverse($order)
    return $order
}

function ConvertTo-ServiceConfig {
    param($Data)

    $name = (Get-StringProperty $Data 'name').Trim()
    $display = (Get-StringProperty $Data 'displayName').Trim()
    if ([string]::IsNullOrWhiteSpace($display)) { $display = $name }

    $serviceType = (Get-StringProperty $Data 'serviceType' 'nssm').Trim().ToLowerInvariant()
    if ($serviceType -ne 'apache' -and $serviceType -ne 'postgresql') { $serviceType = 'nssm' }

    $startup = (Get-StringProperty $Data 'startup' 'auto').ToLowerInvariant()
    if ($startup -ne 'delayed' -and $startup -ne 'manual') { $startup = 'auto' }

    $restartOnCrash = $true
    if ($null -ne $Data -and $null -ne $Data.PSObject.Properties['restartOnCrash']) {
        $restartOnCrash = [bool]$Data.restartOnCrash
    }

    $restartDelaySec = 5
    if ($null -ne $Data -and $null -ne $Data.PSObject.Properties['restartDelaySec']) {
        $rawDelay = $Data.restartDelaySec
        if ($null -ne $rawDelay -and ([string]$rawDelay) -match '^\d+$') {
            $restartDelaySec = [int]$rawDelay
            if ($restartDelaySec -lt 1) { $restartDelaySec = 1 }
        }
    }

    $deps = @()
    if ($null -ne $Data -and $null -ne $Data.PSObject.Properties['dependencies']) {
        $rawDeps = $Data.dependencies
        if ($rawDeps -is [string]) {
            $deps = @(Split-Dependencies $rawDeps)
        } elseif ($null -ne $rawDeps) {
            foreach ($dep in @($rawDeps)) {
                $value = ([string]$dep).Trim()
                if ($value) { $deps += $value }
            }
        }
    }

    $addedAt = (Get-StringProperty $Data 'addedAt').Trim()
    if ([string]::IsNullOrWhiteSpace($addedAt)) { $addedAt = (Get-Date).ToString('o') }
    $updatedAt = (Get-StringProperty $Data 'updatedAt').Trim()
    if ([string]::IsNullOrWhiteSpace($updatedAt)) { $updatedAt = $addedAt }

    $workingDirectory = (Get-StringProperty $Data 'workingDirectory').Trim()
    $apacheRoot = (Get-StringProperty $Data 'apacheRoot').Trim()
    $apacheConfig = (Get-StringProperty $Data 'apacheConfig').Trim()
    $pgData = (Get-StringProperty $Data 'pgData').Trim()
    $pgAccount = (Get-StringProperty $Data 'pgAccount').Trim()

    if ($serviceType -eq 'apache') {
        if ([string]::IsNullOrWhiteSpace($apacheConfig)) { $apacheConfig = 'conf/httpd.conf' }
        if ([string]::IsNullOrWhiteSpace($workingDirectory) -and $apacheRoot) { $workingDirectory = $apacheRoot }
    } elseif ($serviceType -eq 'postgresql') {
        if ([string]::IsNullOrWhiteSpace($pgAccount)) { $pgAccount = 'NT AUTHORITY\NetworkService' }
        if ([string]::IsNullOrWhiteSpace($workingDirectory) -and $pgData) { $workingDirectory = $pgData }
    }

    return [pscustomobject]@{
        name = $name
        displayName = $display
        serviceType = $serviceType
        executable = (Get-StringProperty $Data 'executable').Trim()
        arguments = Get-StringProperty $Data 'arguments'
        workingDirectory = $workingDirectory
        apacheRoot = $apacheRoot
        apacheConfig = $apacheConfig
        pgData = $pgData
        pgAccount = $pgAccount
        stdoutLog = (Get-StringProperty $Data 'stdoutLog').Trim()
        stderrLog = (Get-StringProperty $Data 'stderrLog').Trim()
        startup = $startup
        restartOnCrash = $restartOnCrash
        restartDelaySec = $restartDelaySec
        dependencies = @($deps)
        addedAt = $addedAt
        updatedAt = $updatedAt
    }
}

function Read-Config {
    $services = New-Object System.Collections.ArrayList

    if (-not (Test-Path -LiteralPath $script:ConfigPath)) {
        return ,$services
    }

    try {
        $raw = Get-Content -LiteralPath $script:ConfigPath -Raw -Encoding UTF8
        if ([string]::IsNullOrWhiteSpace($raw)) { return ,$services }

        $data = $raw | ConvertFrom-Json
        $items = @()
        if ($data -is [array]) {
            $items = @($data)
        } elseif ($null -ne $data -and $null -ne $data.PSObject.Properties['services']) {
            $items = @($data.services)
        }

        foreach ($item in $items) {
            if ($null -eq $item) { continue }
            $cfg = ConvertTo-ServiceConfig $item
            if (-not [string]::IsNullOrWhiteSpace($cfg.name)) {
                [void]$services.Add($cfg)
            }
        }

        return ,$services
    } catch {
        Write-AppLog "Config tidak valid: $($_.Exception.Message)"
        $backupName = 'services-broken-{0}.json' -f (Get-Date -Format 'yyyyMMdd-HHmmss')
        try {
            Copy-Item -LiteralPath $script:ConfigPath -Destination (Join-Path $script:BackupDir $backupName) -Force
        } catch {
        }
        [System.Windows.Forms.MessageBox]::Show("File services.json tidak bisa dibaca.`r`n`r`n$($_.Exception.Message)`r`n`r`nFile lama dicadangkan ke folder backup. Aplikasi dimulai dengan daftar kosong.", 'Service Manager', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
        return ,$services
    }
}

function Get-ConfigJson {
    $payload = [pscustomobject]@{
        version = 1
        services = @($script:Services.ToArray())
    }
    return ($payload | ConvertTo-Json -Depth 8)
}

function Backup-Config {
    if (-not (Test-Path -LiteralPath $script:ConfigPath)) { return }
    try {
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss-fff'
        $dest = Join-Path $script:BackupDir ("services-$stamp.json")
        Copy-Item -LiteralPath $script:ConfigPath -Destination $dest -Force
        $oldFiles = Get-ChildItem -LiteralPath $script:BackupDir -Filter 'services-*.json' -File | Sort-Object LastWriteTime -Descending | Select-Object -Skip 20
        foreach ($file in $oldFiles) {
            Remove-Item -LiteralPath $file.FullName -Force -ErrorAction SilentlyContinue
        }
    } catch {
        Write-AppLog "Backup config gagal: $($_.Exception.Message)"
    }
}

function Save-Config {
    Backup-Config
    $json = Get-ConfigJson
    $tmpPath = $script:ConfigPath + '.tmp'
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($tmpPath, $json, $utf8)
    Move-Item -LiteralPath $tmpPath -Destination $script:ConfigPath -Force
    Write-AppLog "Config disimpan ($($script:Services.Count) service)."
}

function Remove-ServiceFromList {
    param([string]$Name)
    for ($i = $script:Services.Count - 1; $i -ge 0; $i--) {
        if ([string]::Equals([string]$script:Services[$i].name, $Name, [System.StringComparison]::OrdinalIgnoreCase)) {
            $script:Services.RemoveAt($i)
        }
    }
}

function Invoke-Nssm {
    param(
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [switch]$IgnoreExitCode
    )

    if (-not (Test-Path -LiteralPath $script:NssmPath)) {
        throw "NSSM tidak ditemukan di $($script:NssmPath)"
    }

    $previousEncoding = $null
    try {
        $previousEncoding = [Console]::OutputEncoding
        [Console]::OutputEncoding = [System.Text.Encoding]::Unicode
    } catch {
        $previousEncoding = $null
    }

    try {
        $output = & $script:NssmPath @Arguments 2>&1
        $code = $LASTEXITCODE
    } finally {
        if ($null -ne $previousEncoding) {
            try {
                [Console]::OutputEncoding = $previousEncoding
            } catch {
            }
        }
    }

    $text = (($output | ForEach-Object { $_.ToString() }) -join "`r`n").Trim()

    if ($code -ne 0 -and -not $IgnoreExitCode) {
        throw "Perintah nssm gagal (exit $code): nssm $($Arguments -join ' ')`r`n$text"
    }

    return [pscustomobject]@{
        ExitCode = $code
        Output = $text
    }
}

function Get-NssmValue {
    param([string]$ServiceName, [string]$Parameter, [string]$SubParameter)
    $nssmArgs = @('get', $ServiceName, $Parameter)
    if (-not [string]::IsNullOrWhiteSpace($SubParameter)) { $nssmArgs += $SubParameter }
    $result = Invoke-Nssm -Arguments $nssmArgs -IgnoreExitCode
    if ($result.ExitCode -ne 0) { return $null }
    $value = $result.Output.Trim()
    if ($value -eq '') { return $null }
    return $value
}

function Set-NssmValue {
    param([string]$ServiceName, [string]$Parameter, [string]$Value)
    if ([string]::IsNullOrEmpty($Value)) {
        Invoke-Nssm -Arguments @('reset', $ServiceName, $Parameter) -IgnoreExitCode | Out-Null
    } else {
        Invoke-Nssm -Arguments @('set', $ServiceName, $Parameter, $Value) | Out-Null
    }
}

function Set-ServiceDelayedStart {
    param([string]$Name, [bool]$Enabled)

    if (-not ('ServiceDelayHelper' -as [type])) {
        throw 'Helper delayed start tidak tersedia.'
    }

    try {
        [ServiceDelayHelper]::SetDelayed($Name, $Enabled)
    } catch {
        $inner = $_.Exception.InnerException
        if ($null -ne $inner) { throw $inner.Message }
        throw
    }
}

function Apply-NativeServiceConfig {
    param($Svc)

    if (-not (Test-ServiceExists $Svc.name)) {
        throw "Service '$($Svc.name)' belum terpasang."
    }

    $displayName = [string]$Svc.displayName
    if ([string]::IsNullOrWhiteSpace($displayName)) { $displayName = [string]$Svc.name }
    Invoke-ScCommand -Arguments @('config', [string]$Svc.name, 'DisplayName=', $displayName) | Out-Null

    $startMode = 'auto'
    if ($Svc.startup -eq 'manual') { $startMode = 'demand' }
    Invoke-ScCommand -Arguments @('config', [string]$Svc.name, 'start=', $startMode) | Out-Null

    if ('ServiceDelayHelper' -as [type]) {
        Set-ServiceDelayedStart -Name $Svc.name -Enabled ($Svc.startup -eq 'delayed')
    } elseif ($Svc.startup -eq 'delayed') {
        throw 'Helper delayed start tidak tersedia; startup delayed tidak bisa diterapkan.'
    }

    $deps = @()
    if ($null -ne $Svc.dependencies) {
        foreach ($dep in @($Svc.dependencies)) {
            $value = ([string]$dep).Trim()
            if ($value -and -not [string]::Equals($value, [string]$Svc.name, [System.StringComparison]::OrdinalIgnoreCase)) {
                $deps += $value
            }
        }
    }
    $deps = @($deps | Select-Object -Unique)

    if ($deps.Count -gt 0) {
        Invoke-ScCommand -Arguments @('config', [string]$Svc.name, 'depend=', ($deps -join '/')) | Out-Null
    } else {
        Invoke-ScCommand -Arguments @('config', [string]$Svc.name, 'depend=', '/') | Out-Null
    }

    if ($Svc.restartOnCrash) {
        $delayMs = [int]$Svc.restartDelaySec * 1000
        if ($delayMs -lt 1000) { $delayMs = 1000 }
        $failureActions = 'restart/{0}/restart/{1}/restart/{2}' -f $delayMs, ($delayMs * 2), ($delayMs * 4)
        Invoke-ScCommand -Arguments @('failure', [string]$Svc.name, 'reset=', '86400', 'actions=', $failureActions) | Out-Null
    } else {
        try {
            Invoke-ScCommand -Arguments @('failure', [string]$Svc.name, 'reset=', '0', 'actions=', '/') | Out-Null
        } catch {
            Write-AppLog "Gagal menghapus recovery action untuk $($Svc.name): $($_.Exception.Message)"
        }
    }

    Write-AppLog "Terapkan config native: $($Svc.name)"
}

function Apply-ServiceConfig {
    param($Svc)

    if (Test-NativeServiceType ([string]$Svc.serviceType)) {
        Apply-NativeServiceConfig $Svc
        return
    }

    $exe = [string]$Svc.executable
    if ([string]::IsNullOrWhiteSpace($exe)) {
        throw "Executable untuk service '$($Svc.name)' belum diisi."
    }

    $workDir = [string]$Svc.workingDirectory
    if ([string]::IsNullOrWhiteSpace($workDir)) { $workDir = Split-Path -Parent $exe }

    $displayName = [string]$Svc.displayName
    if ([string]::IsNullOrWhiteSpace($displayName)) { $displayName = [string]$Svc.name }

    Set-NssmValue $Svc.name 'Application' $exe
    Set-NssmValue $Svc.name 'AppDirectory' $workDir
    Set-NssmValue $Svc.name 'DisplayName' $displayName
    Set-NssmValue $Svc.name 'AppParameters' ([string]$Svc.arguments)

    $outLog = [string]$Svc.stdoutLog
    $errLog = [string]$Svc.stderrLog
    if ($outLog -and -not $errLog) { $errLog = $outLog }
    if (-not $outLog -and $errLog) { $outLog = $errLog }

    if ($outLog) {
        Set-NssmValue $Svc.name 'AppStdout' $outLog
        Set-NssmValue $Svc.name 'AppStderr' $errLog
        Set-NssmValue $Svc.name 'AppRotateFiles' '1'
        Set-NssmValue $Svc.name 'AppRotateOnline' '1'
        Set-NssmValue $Svc.name 'AppRotateBytes' '10485760'
    } else {
        Set-NssmValue $Svc.name 'AppStdout' ''
        Set-NssmValue $Svc.name 'AppStderr' ''
    }

    $startValue = 'SERVICE_AUTO_START'
    if ($Svc.startup -eq 'manual') { $startValue = 'SERVICE_DEMAND_START' }
    Set-NssmValue $Svc.name 'Start' $startValue

    if ('ServiceDelayHelper' -as [type]) {
        Set-ServiceDelayedStart -Name $Svc.name -Enabled ($Svc.startup -eq 'delayed')
    } elseif ($Svc.startup -eq 'delayed') {
        Set-NssmValue $Svc.name 'Start' 'SERVICE_DELAYED_AUTO_START'
    }

    if ($Svc.restartOnCrash) {
        Invoke-Nssm -Arguments @('set', $Svc.name, 'AppExit', 'Default', 'Restart') | Out-Null
        Set-NssmValue $Svc.name 'AppRestartDelay' ([string]([int]$Svc.restartDelaySec * 1000))
        Set-NssmValue $Svc.name 'AppThrottle' '5000'
        try {
            $failureOutput = & sc.exe failure $Svc.name 'reset=' '86400' 'actions=' 'restart/5000/restart/10000/restart/30000' 2>&1
            if ($LASTEXITCODE -ne 0) {
                Write-AppLog "Gagal set recovery action untuk $($Svc.name): $(($failureOutput | ForEach-Object { $_.ToString() }) -join ' ')"
            }
        } catch {
            Write-AppLog "Gagal set recovery action untuk $($Svc.name): $($_.Exception.Message)"
        }
    } else {
        Invoke-Nssm -Arguments @('set', $Svc.name, 'AppExit', 'Default', 'Exit') | Out-Null
    }

    $deps = @()
    if ($null -ne $Svc.dependencies) {
        foreach ($dep in @($Svc.dependencies)) {
            $value = ([string]$dep).Trim()
            if ($value) { $deps += $value }
        }
    }

    if ($deps.Count -gt 0) {
        $setArgs = @('set', $Svc.name, 'DependOnService') + $deps
        Invoke-Nssm -Arguments $setArgs | Out-Null
    } else {
        Invoke-Nssm -Arguments @('reset', $Svc.name, 'DependOnService') -IgnoreExitCode | Out-Null
    }
}

function Test-ServiceExists {
    param([string]$Name)
    if ([string]::IsNullOrWhiteSpace($Name)) { return $false }
    return $null -ne (Get-Service -Name $Name -ErrorAction SilentlyContinue)
}

function Get-ServiceStatus {
    param([string]$Name)
    $svc = Get-Service -Name $Name -ErrorAction SilentlyContinue
    if ($null -eq $svc) { return 'Not Installed' }
    return $svc.Status.ToString()
}

function Wait-ServiceState {
    param([string]$Name, [string]$Desired, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    $attempts = 0
    while ((Get-Date) -lt $deadline) {
        $current = Get-ServiceStatus -Name $Name
        if ($current -eq $Desired) { return $true }
        $attempts++
        if ($Desired -eq 'Running' -and $attempts -gt 6 -and $current -eq 'Stopped') {
            return $false
        }
        Start-Sleep -Milliseconds 400
    }
    return $false
}

function Get-StatusMap {
    $map = @{}
    $names = @()
    foreach ($svc in $script:Services) {
        $names += [string]$svc.name
        $map[[string]$svc.name] = 'Not Installed'
    }
    if ($names.Count -eq 0) { return $map }

    try {
        $found = @(Get-Service -Name $names -ErrorAction SilentlyContinue)
        foreach ($service in $found) {
            if ($null -ne $service) { $map[$service.Name] = $service.Status.ToString() }
        }
    } catch {
    }
    return $map
}

function Start-ManagedService {
    param([string]$Name)
    if (-not (Test-ServiceExists $Name)) {
        throw "Service '$Name' belum terpasang."
    }

    $status = Get-ServiceStatus -Name $Name
    if ($status -eq 'Running') {
        return
    }

    if (Test-NativeServiceType (Get-ManagedServiceType $Name)) {
        try {
            Start-Service -Name $Name -ErrorAction Stop
        } catch {
            throw "Gagal menjalankan '$Name': $($_.Exception.Message)"
        }
        if (-not (Wait-ServiceState -Name $Name -Desired 'Running' -TimeoutSec 30)) {
            throw "Service '$Name' tidak berjalan (status: $(Get-ServiceStatus -Name $Name)). Cek file log atau Event Viewer."
        }
        Write-AppLog "Start service native: $Name"
        return
    }

    $result = Invoke-Nssm -Arguments @('start', $Name) -IgnoreExitCode
    if ($result.ExitCode -ne 0) {
        $current = Get-ServiceStatus -Name $Name
        if ($current -ne 'Running') {
            throw "Gagal menjalankan '$Name': $($result.Output)"
        }
    }

    if (-not (Wait-ServiceState -Name $Name -Desired 'Running' -TimeoutSec 20)) {
        throw "Service '$Name' tidak berjalan (status: $(Get-ServiceStatus -Name $Name)). Cek file log atau Event Viewer."
    }

    Write-AppLog "Start service: $Name"
}

function Stop-ManagedService {
    param([string]$Name)
    if (-not (Test-ServiceExists $Name)) { return }

    $status = Get-ServiceStatus -Name $Name
    if ($status -eq 'Stopped') { return }

    if (Test-NativeServiceType (Get-ManagedServiceType $Name)) {
        try {
            Stop-Service -Name $Name -ErrorAction Stop
        } catch {
            throw "Gagal menghentikan '$Name': $($_.Exception.Message)"
        }
        if (-not (Wait-ServiceState -Name $Name -Desired 'Stopped' -TimeoutSec 30)) {
            throw "Service '$Name' tidak berhenti (status: $(Get-ServiceStatus -Name $Name))."
        }
        Write-AppLog "Stop service native: $Name"
        return
    }

    $result = Invoke-Nssm -Arguments @('stop', $Name) -IgnoreExitCode
    if ($result.ExitCode -ne 0) {
        $current = Get-ServiceStatus -Name $Name
        if ($current -ne 'Stopped') {
            throw "Gagal menghentikan '$Name': $($result.Output)"
        }
    }

    if (-not (Wait-ServiceState -Name $Name -Desired 'Stopped' -TimeoutSec 25)) {
        throw "Service '$Name' tidak berhenti (status: $(Get-ServiceStatus -Name $Name))."
    }

    Write-AppLog "Stop service: $Name"
}

function Restart-ManagedService {
    param([string]$Name)
    if (-not (Test-ServiceExists $Name)) {
        throw "Service '$Name' belum terpasang."
    }

    if (Test-NativeServiceType (Get-ManagedServiceType $Name)) {
        if ((Get-ServiceStatus -Name $Name) -ne 'Stopped') {
            Stop-ManagedService $Name
        }
        Start-ManagedService $Name
        Write-AppLog "Restart service native: $Name"
        return
    }

    $result = Invoke-Nssm -Arguments @('restart', $Name) -IgnoreExitCode
    if ($result.ExitCode -ne 0) {
        $current = Get-ServiceStatus -Name $Name
        if ($current -ne 'Running') {
            throw "Gagal me-restart '$Name': $($result.Output)"
        }
    }

    if (-not (Wait-ServiceState -Name $Name -Desired 'Running' -TimeoutSec 30)) {
        throw "Service '$Name' tidak berjalan setelah restart (status: $(Get-ServiceStatus -Name $Name))."
    }

    Write-AppLog "Restart service: $Name"
}

function Remove-ManagedServiceFromWindows {
    param([string]$Name)
    if (-not (Test-ServiceExists $Name)) { return }

    $type = Get-ManagedServiceType $Name
    if (Test-NativeServiceType $type) {
        $cfg = Get-ServiceConfigByName $Name
        if ($null -eq $cfg) {
            throw "Config untuk service native '$Name' tidak ditemukan. Uninstall dibatalkan."
        }
        Assert-NativeServiceConfig -Svc $cfg -ForRemove
        Stop-ManagedService $Name
        $plan = Get-NativeInstallPlan $cfg
        Invoke-ExternalCommand -FilePath $plan.FilePath -Arguments $plan.RemoveArguments | Out-Null

        $deadline = (Get-Date).AddSeconds(20)
        while ((Get-Date) -lt $deadline -and (Test-ServiceExists $Name)) {
            Start-Sleep -Milliseconds 400
        }

        if (Test-ServiceExists $Name) {
            throw "Service '$Name' masih ada setelah uninstall."
        }

        Write-AppLog "Remove service native: $Name"
        return
    }

    Stop-ManagedService $Name
    Invoke-Nssm -Arguments @('remove', $Name, 'confirm') | Out-Null

    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline -and (Test-ServiceExists $Name)) {
        Start-Sleep -Milliseconds 400
    }

    if (Test-ServiceExists $Name) {
        throw "Service '$Name' masih ada setelah perintah remove."
    }

    Write-AppLog "Remove service: $Name"
}

function Install-ManagedService {
    param($Svc, [switch]$StartAfter)

    if (Test-ServiceExists $Svc.name) {
        throw "Service '$($Svc.name)' sudah ada di Windows. Gunakan Import atau Edit."
    }
    if (-not (Test-Path -LiteralPath $Svc.executable -PathType Leaf)) {
        throw "File aplikasi tidak ditemukan: $($Svc.executable)"
    }

    $type = [string]$Svc.serviceType
    if (Test-NativeServiceType $type) {
        Assert-NativeServiceConfig -Svc $Svc
        Write-AppLog "Install service native: $($Svc.name) [$type]"
        try {
            $plan = Get-NativeInstallPlan $Svc
            Invoke-ExternalCommand -FilePath $plan.FilePath -Arguments $plan.Arguments | Out-Null

            $deadline = (Get-Date).AddSeconds(20)
            while ((Get-Date) -lt $deadline -and -not (Test-ServiceExists $Svc.name)) {
                Start-Sleep -Milliseconds 400
            }
            if (-not (Test-ServiceExists $Svc.name)) {
                throw "Service '$($Svc.name)' tidak muncul setelah proses install."
            }

            Apply-ServiceConfig $Svc
        } catch {
            if (Test-ServiceExists $Svc.name) {
                try { Stop-ManagedService $Svc.name } catch { }
                try {
                    $removePlan = Get-NativeInstallPlan $Svc
                    Invoke-ExternalCommand -FilePath $removePlan.FilePath -Arguments $removePlan.RemoveArguments -IgnoreExitCode | Out-Null
                } catch { }
            }
            throw
        }

        if ($StartAfter) {
            Start-ManagedService $Svc.name
        }
        Write-AppLog "Install service native selesai: $($Svc.name)"
        return
    }

    Write-AppLog "Install service: $($Svc.name) -> $($Svc.executable)"

    try {
        Invoke-Nssm -Arguments @('install', $Svc.name, $Svc.executable) | Out-Null
        Start-Sleep -Milliseconds 500
        Apply-ServiceConfig $Svc
    } catch {
        try {
            Invoke-Nssm -Arguments @('remove', $Svc.name, 'confirm') -IgnoreExitCode | Out-Null
        } catch {
        }
        throw
    }

    if ($StartAfter) {
        Start-ManagedService $Svc.name
    }
}

function Get-ServiceStartupFromCim {
    param($CimService)
    $startup = 'auto'
    $startMode = [string]$CimService.StartMode
    $delayedStart = $false
    if ($null -ne $CimService.PSObject.Properties['DelayedAutoStart']) {
        $delayedStart = [bool]$CimService.DelayedAutoStart
    }
    if ($startMode -eq 'Manual' -or $startMode -eq 'Disabled') {
        $startup = 'manual'
    } elseif ($delayedStart) {
        $startup = 'delayed'
    }
    return $startup
}

function Get-ServiceDependenciesFromCim {
    param($CimService)
    $deps = @()
    foreach ($dep in @($CimService.ServicesDependedOn)) {
        if ($null -ne $dep -and -not [string]::IsNullOrWhiteSpace([string]$dep.Name)) {
            $deps += [string]$dep.Name
        }
    }
    return $deps
}

function ConvertFrom-NssmService {
    param($CimService)

    $name = [string]$CimService.Name
    $exe = Get-NssmValue -ServiceName $name -Parameter 'Application'
    if ([string]::IsNullOrWhiteSpace($exe)) { $exe = '' }

    $appArgs = Get-NssmValue -ServiceName $name -Parameter 'AppParameters'
    if ($null -eq $appArgs) { $appArgs = '' }
    $workDir = Get-NssmValue -ServiceName $name -Parameter 'AppDirectory'
    if ($null -eq $workDir) { $workDir = '' }
    $outLog = Get-NssmValue -ServiceName $name -Parameter 'AppStdout'
    if ($null -eq $outLog) { $outLog = '' }
    $errLog = Get-NssmValue -ServiceName $name -Parameter 'AppStderr'
    if ($null -eq $errLog) { $errLog = '' }

    $startup = Get-ServiceStartupFromCim $CimService

    $exitRaw = Get-NssmValue -ServiceName $name -Parameter 'AppExit' -SubParameter 'Default'
    $restartOnCrash = $true
    if ($exitRaw -and $exitRaw -match '(?i)^Exit$') { $restartOnCrash = $false }

    $delayRaw = Get-NssmValue -ServiceName $name -Parameter 'AppRestartDelay'
    $delay = 5
    if ($delayRaw -match '^\d+$') {
        $delay = [int][Math]::Round(([int]$delayRaw) / 1000.0)
        if ($delay -lt 1) { $delay = 1 }
    }

    $deps = @(Split-Dependencies (Get-NssmValue -ServiceName $name -Parameter 'DependOnService'))

    $display = [string]$CimService.DisplayName
    if ([string]::IsNullOrWhiteSpace($display)) { $display = $name }

    return ConvertTo-ServiceConfig ([pscustomobject]@{
        name = $name
        displayName = $display
        serviceType = 'nssm'
        executable = $exe
        arguments = $appArgs
        workingDirectory = $workDir
        stdoutLog = $outLog
        stderrLog = $errLog
        startup = $startup
        restartOnCrash = $restartOnCrash
        restartDelaySec = $delay
        dependencies = @($deps)
        addedAt = (Get-Date).ToString('o')
        updatedAt = (Get-Date).ToString('o')
    })
}

function ConvertFrom-ApacheService {
    param($CimService)

    $name = [string]$CimService.Name
    $pathName = [string]$CimService.PathName
    $exe = Get-ExecutableFromPathName $pathName
    $root = Get-CommandLineArgument -CommandLine $pathName -Name '-d'
    if ([string]::IsNullOrWhiteSpace($root)) { $root = Get-ApacheRootFromExe $exe }
    $config = Get-CommandLineArgument -CommandLine $pathName -Name '-f'
    if ([string]::IsNullOrWhiteSpace($config)) { $config = 'conf/httpd.conf' }

    $outLog = ''
    $errLog = ''
    if (-not [string]::IsNullOrWhiteSpace($root)) {
        $logDir = Join-Path $root 'logs'
        $candidate = Join-Path $logDir 'access.log'
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { $outLog = $candidate }
        $candidate = Join-Path $logDir 'error.log'
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { $errLog = $candidate }
    }

    $display = [string]$CimService.DisplayName
    if ([string]::IsNullOrWhiteSpace($display)) { $display = $name }

    return ConvertTo-ServiceConfig ([pscustomobject]@{
        name = $name
        displayName = $display
        serviceType = 'apache'
        executable = $exe
        arguments = ''
        workingDirectory = $root
        apacheRoot = $root
        apacheConfig = $config
        stdoutLog = $outLog
        stderrLog = $errLog
        startup = (Get-ServiceStartupFromCim $CimService)
        restartOnCrash = $true
        restartDelaySec = 5
        dependencies = @(Get-ServiceDependenciesFromCim $CimService)
        addedAt = (Get-Date).ToString('o')
        updatedAt = (Get-Date).ToString('o')
    })
}

function ConvertFrom-PostgreSqlService {
    param($CimService)

    $name = [string]$CimService.Name
    $pathName = [string]$CimService.PathName
    $exe = Get-ExecutableFromPathName $pathName
    $pgData = Get-CommandLineArgument -CommandLine $pathName -Name '-D'
    $account = [string]$CimService.StartName
    if ([string]::IsNullOrWhiteSpace($account)) { $account = 'NT AUTHORITY\NetworkService' }

    $log = ''
    if (-not [string]::IsNullOrWhiteSpace($pgData)) {
        $log = Get-NewestLogFile (Join-Path $pgData 'log')
    }

    $display = [string]$CimService.DisplayName
    if ([string]::IsNullOrWhiteSpace($display)) { $display = $name }

    return ConvertTo-ServiceConfig ([pscustomobject]@{
        name = $name
        displayName = $display
        serviceType = 'postgresql'
        executable = $exe
        arguments = ''
        workingDirectory = $pgData
        pgData = $pgData
        pgAccount = $account
        stdoutLog = $log
        stderrLog = $log
        startup = (Get-ServiceStartupFromCim $CimService)
        restartOnCrash = $true
        restartDelaySec = 5
        dependencies = @(Get-ServiceDependenciesFromCim $CimService)
        addedAt = (Get-Date).ToString('o')
        updatedAt = (Get-Date).ToString('o')
    })
}

function Get-ManagedServiceCandidates {
    $result = @()
    $seen = @{}

    try {
        $cimServices = @(Get-CimInstance -ClassName Win32_Service -ErrorAction Stop)
    } catch {
        throw "Gagal membaca daftar service Windows: $($_.Exception.Message)"
    }

    foreach ($cim in $cimServices) {
        if ($null -eq $cim -or [string]::IsNullOrWhiteSpace([string]$cim.PathName)) { continue }
        $pathName = [string]$cim.PathName
        $type = ''
        if ($pathName -match '(?i)nssm\.exe') { $type = 'nssm' }
        elseif ($pathName -match '(?i)httpd\.exe') { $type = 'apache' }
        elseif ($pathName -match '(?i)pg_ctl\.exe') { $type = 'postgresql' }
        else { continue }

        $key = ([string]$cim.Name).ToLowerInvariant()
        if ($seen.ContainsKey($key)) { continue }
        $seen[$key] = $true

        try {
            switch ($type) {
                'apache' { $cfg = ConvertFrom-ApacheService -CimService $cim }
                'postgresql' { $cfg = ConvertFrom-PostgreSqlService -CimService $cim }
                default { $cfg = ConvertFrom-NssmService -CimService $cim }
            }
            if ($null -ne $cfg -and -not [string]::IsNullOrWhiteSpace($cfg.name)) {
                $result += $cfg
            }
        } catch {
            Write-AppLog "Gagal membaca service '$($cim.Name)': $($_.Exception.Message)"
        }
    }

    return $result
}

function Set-StatusCellStyle {
    param($Cell, [string]$Status)
    if ($null -ne $script:StatusFont) { $Cell.Style.Font = $script:StatusFont }
    switch ($Status) {
        'Running' { $Cell.Style.ForeColor = [System.Drawing.Color]::FromArgb(0, 120, 0) }
        'Stopped' { $Cell.Style.ForeColor = [System.Drawing.Color]::FromArgb(180, 0, 0) }
        'Not Installed' { $Cell.Style.ForeColor = [System.Drawing.Color]::Gray }
        'Paused' { $Cell.Style.ForeColor = [System.Drawing.Color]::FromArgb(0, 90, 170) }
        default { $Cell.Style.ForeColor = [System.Drawing.Color]::FromArgb(190, 100, 0) }
    }
}

function Get-SelectedService {
    if ($null -eq $script:Grid -or $script:Grid.SelectedRows.Count -eq 0) { return $null }
    $name = [string]$script:Grid.SelectedRows[0].Tag
    foreach ($svc in $script:Services) {
        if ([string]::Equals([string]$svc.name, $name, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $svc
        }
    }
    return $null
}

function Refresh-Grid {
    if ($null -eq $script:Grid) { return }

    $selectedName = $null
    if ($script:Grid.SelectedRows.Count -gt 0) {
        $selectedName = [string]$script:Grid.SelectedRows[0].Tag
    }

    $script:SuppressEvents = $true
    $script:Grid.SuspendLayout()
    try {
        $script:Grid.Rows.Clear()
        $statusMap = Get-StatusMap

        foreach ($svc in $script:Services) {
            $status = 'Not Installed'
            if ($statusMap.ContainsKey([string]$svc.name)) { $status = [string]$statusMap[[string]$svc.name] }

            $startupText = 'Auto'
            if ($svc.startup -eq 'delayed') { $startupText = 'Auto (Delayed)' }
            elseif ($svc.startup -eq 'manual') { $startupText = 'Manual' }

            $restartText = 'Tidak'
            if ($svc.restartOnCrash) { $restartText = "Ya ($($svc.restartDelaySec)s)" }

            $typeText = Get-ServiceTypeLabel $svc.serviceType
            $rowIndex = $script:Grid.Rows.Add($status, $svc.name, $svc.displayName, $typeText, $svc.executable, $startupText, $restartText)
            $row = $script:Grid.Rows[$rowIndex]
            $row.Tag = [string]$svc.name
            Set-StatusCellStyle $row.Cells[0] $status
        }
    } finally {
        $script:Grid.ResumeLayout()
        $script:SuppressEvents = $false
    }

    if ($selectedName) {
        foreach ($row in $script:Grid.Rows) {
            if ([string]::Equals([string]$row.Tag, $selectedName, [System.StringComparison]::OrdinalIgnoreCase)) {
                $row.Selected = $true
                $script:Grid.CurrentCell = $row.Cells[0]
                break
            }
        }
    }

    if ($script:Grid.Rows.Count -gt 0 -and $script:Grid.SelectedRows.Count -eq 0) {
        $script:Grid.Rows[0].Selected = $true
        $script:Grid.CurrentCell = $script:Grid.Rows[0].Cells[0]
    }

    Update-LogView
    Update-StatusBar
}

function Update-StatusCells {
    if ($null -eq $script:Grid -or $script:Grid.Rows.Count -eq 0) {
        Update-StatusBar
        return
    }

    $statusMap = Get-StatusMap
    foreach ($row in $script:Grid.Rows) {
        $name = [string]$row.Tag
        $status = 'Not Installed'
        if ($statusMap.ContainsKey($name)) { $status = [string]$statusMap[$name] }
        if ([string]$row.Cells[0].Value -ne $status) {
            $row.Cells[0].Value = $status
            Set-StatusCellStyle $row.Cells[0] $status
        }
    }
    Update-StatusBar
}

function Update-StatusBar {
    if ($null -eq $script:Grid -or $null -eq $script:CountLabel) { return }
    $installed = 0
    $running = 0
    foreach ($row in $script:Grid.Rows) {
        $state = [string]$row.Cells[0].Value
        if ($state -ne '' -and $state -ne 'Not Installed') { $installed++ }
        if ($state -eq 'Running') { $running++ }
    }
    $script:CountLabel.Text = '{0} terdaftar | {1} terpasang | {2} berjalan' -f $script:Services.Count, $installed, $running
}

function Read-LogTail {
    param([string]$Path, [int]$MaxBytes = 131072)

    if ([string]::IsNullOrWhiteSpace($Path)) { return '' }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return '(file log belum ada)' }

    $stream = $null
    try {
        $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
        $length = $stream.Length
        if ($length -eq 0) { return '(file log kosong)' }

        $start = [Math]::Max(0, $length - $MaxBytes)
        [void]$stream.Seek($start, [System.IO.SeekOrigin]::Begin)

        $size = [int]($length - $start)
        $buffer = New-Object 'byte[]' $size
        $read = $stream.Read($buffer, 0, $size)
        $text = [System.Text.Encoding]::UTF8.GetString($buffer, 0, $read)

        if ($start -gt 0) {
            $newlineIndex = $text.IndexOf("`n")
            if ($newlineIndex -ge 0) { $text = $text.Substring($newlineIndex + 1) }
        }

        $lines = $text -split '\r?\n'
        if ($lines.Count -gt 600) {
            $lines = $lines[($lines.Count - 600)..($lines.Count - 1)]
        }
        return ($lines -join "`r`n")
    } catch {
        return "(gagal membaca log: $($_.Exception.Message))"
    } finally {
        if ($null -ne $stream) { $stream.Dispose() }
    }
}

function Update-LogView {
    if ($null -eq $script:LogBox) { return }

    $svc = Get-SelectedService
    if ($null -eq $svc) {
        $script:LogLabel.Text = 'Log service: (belum ada yang dipilih)'
        if ($script:LastLogText -ne '') {
            $script:LogBox.Text = ''
            $script:LastLogText = ''
        }
        return
    }

    $paths = @()
    if ($svc.stdoutLog) { $paths += [string]$svc.stdoutLog }
    if ($svc.stderrLog -and -not [string]::Equals([string]$svc.stderrLog, [string]$svc.stdoutLog, [System.StringComparison]::OrdinalIgnoreCase)) {
        $paths += [string]$svc.stderrLog
    }

    if ($paths.Count -eq 0) {
        $text = "(Service '$($svc.name)' belum punya path log. Klik Edit untuk mengatur AppStdout/AppStderr.)"
    } else {
        $chunks = @()
        foreach ($path in $paths) {
            $chunks += "=== $path ==="
            $chunks += (Read-LogTail -Path $path)
            $chunks += ''
        }
        $text = ($chunks -join "`r`n").TrimEnd()
    }

    if ($text -ne $script:LastLogText) {
        $script:LastLogText = $text
        $script:LogBox.Text = $text
        $script:LogBox.SelectionStart = $script:LogBox.TextLength
        $script:LogBox.ScrollToCaret()
    }

    $script:LogLabel.Text = "Log service: $($svc.name)"
}

function Select-LogPath {
    param(
        [System.Windows.Forms.IWin32Window]$Owner,
        [string]$Title,
        [string]$Current,
        [string]$DefaultName
    )

    $dialog = New-Object System.Windows.Forms.SaveFileDialog
    $dialog.Title = $Title
    $dialog.Filter = 'Log (*.log)|*.log|Semua file (*.*)|*.*'
    $dialog.DefaultExt = 'log'
    $dialog.AddExtension = $true
    $dialog.OverwritePrompt = $false

    if (-not [string]::IsNullOrWhiteSpace($Current)) {
        $directory = [System.IO.Path]::GetDirectoryName($Current)
        if ($directory -and (Test-Path -LiteralPath $directory)) { $dialog.InitialDirectory = $directory }
        $dialog.FileName = [System.IO.Path]::GetFileName($Current)
    } else {
        if ([string]::IsNullOrWhiteSpace($DefaultName)) { $DefaultName = 'service.log' }
        $dialog.FileName = $DefaultName
        if (Test-Path -LiteralPath $script:LogDir) { $dialog.InitialDirectory = $script:LogDir }
    }

    if ($dialog.ShowDialog($Owner) -eq [System.Windows.Forms.DialogResult]::OK) {
        return $dialog.FileName
    }
    return $null
}

function Select-ServiceExe {
    if ($null -eq $script:DlgForm) { return }

    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Title = 'Pilih aplikasi service'
    $dialog.Filter = 'Executable (*.exe)|*.exe|Semua file (*.*)|*.*'
    if ($script:DlgExe.Text) {
        $directory = [System.IO.Path]::GetDirectoryName($script:DlgExe.Text)
        if ($directory -and (Test-Path -LiteralPath $directory)) { $dialog.InitialDirectory = $directory }
    }

    if ($dialog.ShowDialog($script:DlgForm) -ne [System.Windows.Forms.DialogResult]::OK) { return }

    $script:DlgExe.Text = $dialog.FileName

    $typeIndex = 0
    if ($null -ne $script:DlgType) { $typeIndex = $script:DlgType.SelectedIndex }

    if ($typeIndex -eq 0) {
        if ([string]::IsNullOrWhiteSpace($script:DlgWork.Text)) {
            $script:DlgWork.Text = Split-Path -Parent $dialog.FileName
        }
        if ([string]::IsNullOrWhiteSpace($script:DlgDisplay.Text)) {
            $script:DlgDisplay.Text = [System.IO.Path]::GetFileNameWithoutExtension($dialog.FileName)
        }
        if ($script:DlgIsNew -and [string]::IsNullOrWhiteSpace($script:DlgName.Text)) {
            $baseName = [System.IO.Path]::GetFileNameWithoutExtension($dialog.FileName)
            $cleanName = $baseName -replace '[^A-Za-z0-9._-]', '-'
            if ($cleanName) { $script:DlgName.Text = $cleanName }
        }
        return
    }

    if ($typeIndex -eq 1) {
        $root = Get-ApacheRootFromExe $dialog.FileName
        if ([string]::IsNullOrWhiteSpace($script:DlgApacheRoot.Text) -and (Test-Path -LiteralPath $root -PathType Container)) {
            $script:DlgApacheRoot.Text = $root
        }
        if ([string]::IsNullOrWhiteSpace($script:DlgApacheConfig.Text)) {
            $script:DlgApacheConfig.Text = 'conf/httpd.conf'
        }
        if ($script:DlgIsNew -and [string]::IsNullOrWhiteSpace($script:DlgName.Text)) {
            $script:DlgName.Text = 'Apache2.4'
        }
        if ([string]::IsNullOrWhiteSpace($script:DlgDisplay.Text)) {
            $script:DlgDisplay.Text = 'Apache 2.4'
        }
    } elseif ($typeIndex -eq 2) {
        if ([string]::IsNullOrWhiteSpace($script:DlgPgAccount.Text)) {
            $script:DlgPgAccount.Text = 'NT AUTHORITY\NetworkService'
        }
        if ($script:DlgIsNew -and [string]::IsNullOrWhiteSpace($script:DlgName.Text)) {
            $script:DlgName.Text = 'postgresql-16'
        }
        if ([string]::IsNullOrWhiteSpace($script:DlgDisplay.Text)) {
            $script:DlgDisplay.Text = 'PostgreSQL 16'
        }
    }
}

function Select-ServiceWorkDir {
    if ($null -eq $script:DlgForm) { return }

    $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
    $dialog.Description = 'Pilih working directory service'
    $dialog.ShowNewFolderButton = $true
    if ($script:DlgWork.Text -and (Test-Path -LiteralPath $script:DlgWork.Text)) {
        $dialog.SelectedPath = $script:DlgWork.Text
    }

    if ($dialog.ShowDialog($script:DlgForm) -eq [System.Windows.Forms.DialogResult]::OK) {
        $script:DlgWork.Text = $dialog.SelectedPath
    }
}

function Select-ServiceStdoutLog {
    if ($null -eq $script:DlgForm) { return }
    $baseName = $script:DlgName.Text
    if ([string]::IsNullOrWhiteSpace($baseName)) { $baseName = 'service' }
    $path = Select-LogPath -Owner $script:DlgForm -Title 'Pilih file log stdout' -Current $script:DlgStdout.Text -DefaultName "$baseName-out.log"
    if ($null -ne $path) {
        $script:DlgStdout.Text = $path
        if ($script:DlgSame.Checked) { $script:DlgStderr.Text = $path }
    }
}

function Select-ServiceStderrLog {
    if ($null -eq $script:DlgForm) { return }
    $baseName = $script:DlgName.Text
    if ([string]::IsNullOrWhiteSpace($baseName)) { $baseName = 'service' }
    $path = Select-LogPath -Owner $script:DlgForm -Title 'Pilih file log stderr' -Current $script:DlgStderr.Text -DefaultName "$baseName-err.log"
    if ($null -ne $path) { $script:DlgStderr.Text = $path }
}

function Update-ServiceLogFields {
    if ($null -eq $script:DlgStdout -or $null -eq $script:DlgStderr) { return }

    if ($null -ne $script:DlgSame -and $script:DlgSame.Checked) {
        if ($script:DlgStderr.Text -ne $script:DlgStdout.Text) {
            $script:DlgStderr.Text = $script:DlgStdout.Text
        }
        $script:DlgStderr.Enabled = $false
        if ($null -ne $script:DlgErrBrowse) { $script:DlgErrBrowse.Enabled = $false }
    } else {
        $script:DlgStderr.Enabled = $true
        if ($null -ne $script:DlgErrBrowse) { $script:DlgErrBrowse.Enabled = $true }
    }
}

function Set-NativeServiceDefaults {
    param([string]$Type)
    if (-not $script:DlgIsNew) { return }

    if ($Type -eq 'apache') {
        $exeDefault = 'C:\laragon\bin\apache\httpd-2.4.63-250122-win64-VS17\bin\httpd.exe'
        if ($null -ne $script:DlgExe -and [string]::IsNullOrWhiteSpace($script:DlgExe.Text) -and (Test-Path -LiteralPath $exeDefault)) {
            $script:DlgExe.Text = $exeDefault
        }
        if ($null -ne $script:DlgName -and [string]::IsNullOrWhiteSpace($script:DlgName.Text)) {
            $script:DlgName.Text = 'Apache2.4'
        }
        if ($null -ne $script:DlgDisplay -and [string]::IsNullOrWhiteSpace($script:DlgDisplay.Text)) {
            $script:DlgDisplay.Text = 'Apache 2.4'
        }

        $exe = [string]$script:DlgExe.Text
        $root = Get-ApacheRootFromExe $exe
        if ($null -ne $script:DlgApacheRoot -and [string]::IsNullOrWhiteSpace($script:DlgApacheRoot.Text) -and (Test-Path -LiteralPath $root -PathType Container)) {
            $script:DlgApacheRoot.Text = $root
        }
        if ($null -ne $script:DlgApacheConfig -and [string]::IsNullOrWhiteSpace($script:DlgApacheConfig.Text)) {
            $script:DlgApacheConfig.Text = 'conf/httpd.conf'
        }

        if ($root) {
            $accessLog = Join-Path $root 'logs\access.log'
            $errorLog = Join-Path $root 'logs\error.log'
            if ($null -ne $script:DlgStdout -and $null -ne $script:DlgStderr -and [string]::IsNullOrWhiteSpace($script:DlgStdout.Text) -and [string]::IsNullOrWhiteSpace($script:DlgStderr.Text) -and (Test-Path -LiteralPath $accessLog) -and (Test-Path -LiteralPath $errorLog)) {
                $script:DlgSame.Checked = $false
                $script:DlgStdout.Text = $accessLog
                $script:DlgStderr.Text = $errorLog
            }
        }

        if ($null -ne $script:DlgDeps -and [string]::IsNullOrWhiteSpace($script:DlgDeps.Text) -and (Test-ServiceExists 'postgresql-16')) {
            $script:DlgDeps.Text = 'postgresql-16'
        }
    } elseif ($Type -eq 'postgresql') {
        $exeDefault = 'C:\laragon\bin\postgresql\postgresql-16.4-1\bin\pg_ctl.exe'
        $dataDefault = 'C:\laragon\data\postgresql-16'
        if ($null -ne $script:DlgExe -and [string]::IsNullOrWhiteSpace($script:DlgExe.Text) -and (Test-Path -LiteralPath $exeDefault)) {
            $script:DlgExe.Text = $exeDefault
        }
        if ($null -ne $script:DlgName -and [string]::IsNullOrWhiteSpace($script:DlgName.Text)) {
            $script:DlgName.Text = 'postgresql-16'
        }
        if ($null -ne $script:DlgDisplay -and [string]::IsNullOrWhiteSpace($script:DlgDisplay.Text)) {
            $script:DlgDisplay.Text = 'PostgreSQL 16'
        }
        if ($null -ne $script:DlgPgData -and [string]::IsNullOrWhiteSpace($script:DlgPgData.Text) -and (Test-Path -LiteralPath $dataDefault -PathType Container)) {
            $script:DlgPgData.Text = $dataDefault
        }
        if ($null -ne $script:DlgPgAccount -and [string]::IsNullOrWhiteSpace($script:DlgPgAccount.Text)) {
            $script:DlgPgAccount.Text = 'NT AUTHORITY\NetworkService'
        }
    }
}

function Update-ServiceTypeFields {
    param([switch]$ApplyDefaults)

    if ($null -eq $script:DlgType) { return }
    $index = $script:DlgType.SelectedIndex
    $isNssm = $index -eq 0
    $isApache = $index -eq 1
    $isPg = $index -eq 2

    foreach ($control in @($script:DlgArgs, $script:DlgLblArgs, $script:DlgWork, $script:DlgLblWork, $script:DlgWorkBrowse)) {
        if ($null -ne $control) { $control.Visible = $isNssm }
    }
    foreach ($control in @($script:DlgApacheRoot, $script:DlgLblApacheRoot, $script:DlgApacheRootBrowse, $script:DlgApacheConfig, $script:DlgLblApacheConfig, $script:DlgApacheConfigBrowse)) {
        if ($null -ne $control) { $control.Visible = $isApache }
    }
    foreach ($control in @($script:DlgPgData, $script:DlgLblPgData, $script:DlgPgDataBrowse, $script:DlgPgAccount, $script:DlgLblPgAccount)) {
        if ($null -ne $control) { $control.Visible = $isPg }
    }

    if ($isNssm) {
        if ($null -ne $script:DlgLblExe) { $script:DlgLblExe.Text = 'Application' }
        if ($null -ne $script:DlgHint) { $script:DlgHint.Text = 'Dependencies dipisahkan spasi atau koma. Untuk file .cmd/.bat: pilih C:\Windows\System32\cmd.exe sebagai Application, lalu isi Arguments: /c "C:\path\file.cmd".' }
    } elseif ($isApache) {
        if ($null -ne $script:DlgLblExe) { $script:DlgLblExe.Text = 'httpd.exe' }
        if ($null -ne $script:DlgHint) { $script:DlgHint.Text = 'Install: httpd.exe -d <Apache root> -f <config> -k install -n <name>. Uninstall: -k uninstall. Perubahan installer berlaku saat Install ulang.' }
    } else {
        if ($null -ne $script:DlgLblExe) { $script:DlgLblExe.Text = 'pg_ctl.exe' }
        if ($null -ne $script:DlgHint) { $script:DlgHint.Text = 'Install/remove memakai pg_ctl register/unregister. PGDATA dan service account dipakai saat install; perubahan berlaku saat Install ulang.' }
    }

    if ($ApplyDefaults) {
        if ($isApache) { Set-NativeServiceDefaults 'apache' }
        elseif ($isPg) { Set-NativeServiceDefaults 'postgresql' }
    }
}

function Select-ApacheRoot {
    if ($null -eq $script:DlgForm) { return }
    $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
    $dialog.Description = 'Pilih Apache root (folder yang berisi conf dan logs)'
    $dialog.ShowNewFolderButton = $false
    if ($script:DlgApacheRoot.Text -and (Test-Path -LiteralPath $script:DlgApacheRoot.Text)) {
        $dialog.SelectedPath = $script:DlgApacheRoot.Text
    }
    if ($dialog.ShowDialog($script:DlgForm) -eq [System.Windows.Forms.DialogResult]::OK) {
        $script:DlgApacheRoot.Text = $dialog.SelectedPath
        if ([string]::IsNullOrWhiteSpace($script:DlgApacheConfig.Text)) {
            $script:DlgApacheConfig.Text = 'conf/httpd.conf'
        }
    }
}

function Select-ApacheConfig {
    if ($null -eq $script:DlgForm) { return }
    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Title = 'Pilih file config Apache'
    $dialog.Filter = 'Config (*.conf;*.txt)|*.conf;*.txt|Semua file (*.*)|*.*'
    $root = [string]$script:DlgApacheRoot.Text
    if ($root) {
        $confDir = Join-Path $root 'conf'
        if (Test-Path -LiteralPath $confDir) { $dialog.InitialDirectory = $confDir }
    }
    if ($dialog.ShowDialog($script:DlgForm) -eq [System.Windows.Forms.DialogResult]::OK) {
        $script:DlgApacheConfig.Text = $dialog.FileName
    }
}

function Select-PostgreSqlData {
    if ($null -eq $script:DlgForm) { return }
    $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
    $dialog.Description = 'Pilih folder PGDATA PostgreSQL'
    $dialog.ShowNewFolderButton = $false
    if ($script:DlgPgData.Text -and (Test-Path -LiteralPath $script:DlgPgData.Text)) {
        $dialog.SelectedPath = $script:DlgPgData.Text
    }
    if ($dialog.ShowDialog($script:DlgForm) -eq [System.Windows.Forms.DialogResult]::OK) {
        $script:DlgPgData.Text = $dialog.SelectedPath
    }
}

function Save-ServiceDialog {
    if ($null -eq $script:DlgForm) { return }

    $newName = $script:DlgName.Text.Trim()
    if ($newName -notmatch '^[A-Za-z0-9._-]{1,256}$') {
        [System.Windows.Forms.MessageBox]::Show($script:DlgForm, 'Nama service hanya boleh berisi huruf, angka, titik, garis bawah, dan tanda minus (1-256 karakter).', 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
        $script:DlgName.Focus()
        return
    }

    $newExe = $script:DlgExe.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($newExe)) {
        [System.Windows.Forms.MessageBox]::Show($script:DlgForm, 'Path aplikasi wajib diisi.', 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
        $script:DlgExe.Focus()
        return
    }
    if (-not [System.IO.Path]::IsPathRooted($newExe)) {
        [System.Windows.Forms.MessageBox]::Show($script:DlgForm, 'Path aplikasi harus lengkap, contoh: C:\folder\app.exe', 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
        $script:DlgExe.Focus()
        return
    }
    if (-not (Test-Path -LiteralPath $newExe -PathType Leaf)) {
        [System.Windows.Forms.MessageBox]::Show($script:DlgForm, "File aplikasi tidak ditemukan:`r`n$newExe", 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
        $script:DlgExe.Focus()
        return
    }

    $typeIndex = 0
    if ($null -ne $script:DlgType) { $typeIndex = $script:DlgType.SelectedIndex }
    $serviceType = 'nssm'
    if ($typeIndex -eq 1) { $serviceType = 'apache' }
    elseif ($typeIndex -eq 2) { $serviceType = 'postgresql' }

    $apacheRoot = ''
    $apacheConfig = ''
    $pgData = ''
    $pgAccount = ''
    $newArgs = ''

    if ($serviceType -eq 'apache') {
        if ([System.IO.Path]::GetFileName($newExe) -ine 'httpd.exe') {
            [System.Windows.Forms.MessageBox]::Show($script:DlgForm, 'Untuk service Apache, Application harus file httpd.exe.', 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
            $script:DlgExe.Focus()
            return
        }

        $apacheRoot = $script:DlgApacheRoot.Text.Trim()
        if ([string]::IsNullOrWhiteSpace($apacheRoot) -or -not [System.IO.Path]::IsPathRooted($apacheRoot)) {
            [System.Windows.Forms.MessageBox]::Show($script:DlgForm, 'Apache root wajib diisi dengan path lengkap.', 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
            $script:DlgApacheRoot.Focus()
            return
        }
        if (-not (Test-Path -LiteralPath $apacheRoot -PathType Container)) {
            [System.Windows.Forms.MessageBox]::Show($script:DlgForm, "Apache root tidak ditemukan:`r`n$apacheRoot", 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
            $script:DlgApacheRoot.Focus()
            return
        }

        $apacheConfig = $script:DlgApacheConfig.Text.Trim()
        if ([string]::IsNullOrWhiteSpace($apacheConfig)) { $apacheConfig = 'conf/httpd.conf' }
        $resolvedConfig = $apacheConfig
        if (-not [System.IO.Path]::IsPathRooted($resolvedConfig)) {
            $resolvedConfig = Join-Path $apacheRoot $resolvedConfig
        }
        if (-not (Test-Path -LiteralPath $resolvedConfig -PathType Leaf)) {
            [System.Windows.Forms.MessageBox]::Show($script:DlgForm, "Config Apache tidak ditemukan:`r`n$resolvedConfig", 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
            $script:DlgApacheConfig.Focus()
            return
        }

        $newWork = $apacheRoot
    } elseif ($serviceType -eq 'postgresql') {
        if ([System.IO.Path]::GetFileName($newExe) -ine 'pg_ctl.exe') {
            [System.Windows.Forms.MessageBox]::Show($script:DlgForm, 'Untuk service PostgreSQL, Application harus file pg_ctl.exe.', 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
            $script:DlgExe.Focus()
            return
        }

        $pgData = $script:DlgPgData.Text.Trim()
        if ([string]::IsNullOrWhiteSpace($pgData) -or -not [System.IO.Path]::IsPathRooted($pgData)) {
            [System.Windows.Forms.MessageBox]::Show($script:DlgForm, 'PGDATA wajib diisi dengan path lengkap.', 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
            $script:DlgPgData.Focus()
            return
        }
        if (-not (Test-Path -LiteralPath $pgData -PathType Container)) {
            [System.Windows.Forms.MessageBox]::Show($script:DlgForm, "PGDATA tidak ditemukan:`r`n$pgData", 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
            $script:DlgPgData.Focus()
            return
        }

        $pgAccount = $script:DlgPgAccount.Text.Trim()
        if ([string]::IsNullOrWhiteSpace($pgAccount)) {
            [System.Windows.Forms.MessageBox]::Show($script:DlgForm, 'Service account PostgreSQL wajib diisi.', 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
            $script:DlgPgAccount.Focus()
            return
        }

        $newWork = $pgData
    } else {
        $newWork = $script:DlgWork.Text.Trim()
        if ([string]::IsNullOrWhiteSpace($newWork)) { $newWork = Split-Path -Parent $newExe }
        if (-not [System.IO.Path]::IsPathRooted($newWork)) {
            [System.Windows.Forms.MessageBox]::Show($script:DlgForm, 'Working directory harus berupa path lengkap.', 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
            $script:DlgWork.Focus()
            return
        }
        if (-not (Test-Path -LiteralPath $newWork -PathType Container)) {
            [System.Windows.Forms.MessageBox]::Show($script:DlgForm, "Working directory tidak ditemukan:`r`n$newWork", 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
            $script:DlgWork.Focus()
            return
        }
        $newArgs = $script:DlgArgs.Text.Trim()
    }

    $newDisplay = $script:DlgDisplay.Text.Trim()
    if ([string]::IsNullOrWhiteSpace($newDisplay)) { $newDisplay = $newName }

    $newOut = $script:DlgStdout.Text.Trim()
    $newErr = $script:DlgStderr.Text.Trim()
    if ($script:DlgSame.Checked) { $newErr = $newOut }
    if ($newOut -and -not [System.IO.Path]::IsPathRooted($newOut)) { $newOut = Join-Path $newWork $newOut }
    if ($newErr -and -not [System.IO.Path]::IsPathRooted($newErr)) { $newErr = Join-Path $newWork $newErr }

    try {
        foreach ($logPath in @($newOut, $newErr)) {
            if ($logPath) {
                $logDirPath = [System.IO.Path]::GetDirectoryName($logPath)
                if ($logDirPath -and -not (Test-Path -LiteralPath $logDirPath)) {
                    New-Item -ItemType Directory -Path $logDirPath -Force | Out-Null
                }
            }
        }
    } catch {
        [System.Windows.Forms.MessageBox]::Show($script:DlgForm, "Gagal membuat folder log:`r`n$($_.Exception.Message)", 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
        return
    }

    if ($script:DlgIsNew) {
        foreach ($existing in $script:Services) {
            if ([string]::Equals([string]$existing.name, $newName, [System.StringComparison]::OrdinalIgnoreCase)) {
                [System.Windows.Forms.MessageBox]::Show($script:DlgForm, "Service '$newName' sudah ada di daftar aplikasi.", 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
                $script:DlgName.Focus()
                return
            }
        }
        if (Test-ServiceExists $newName) {
            [System.Windows.Forms.MessageBox]::Show($script:DlgForm, "Service '$newName' sudah terpasang di Windows. Gunakan tombol Import untuk memasukkannya ke daftar.", 'Validasi', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Warning) | Out-Null
            $script:DlgName.Focus()
            return
        }
    }

    $startup = 'auto'
    if ($script:DlgStartup.SelectedIndex -eq 1) { $startup = 'delayed' }
    elseif ($script:DlgStartup.SelectedIndex -eq 2) { $startup = 'manual' }

    $deps = @(Split-Dependencies $script:DlgDeps.Text | Select-Object -Unique)
    $deps = @($deps | Where-Object { -not [string]::Equals([string]$_, $newName, [System.StringComparison]::OrdinalIgnoreCase) })

    $addedAt = (Get-Date).ToString('o')
    if ($null -ne $script:DlgService -and $script:DlgService.addedAt) {
        $addedAt = [string]$script:DlgService.addedAt
    }

    $newService = [pscustomobject]@{
        name = $newName
        displayName = $newDisplay
        serviceType = $serviceType
        executable = $newExe
        arguments = $newArgs
        workingDirectory = $newWork
        apacheRoot = $apacheRoot
        apacheConfig = $apacheConfig
        pgData = $pgData
        pgAccount = $pgAccount
        stdoutLog = $newOut
        stderrLog = $newErr
        startup = $startup
        restartOnCrash = [bool]$script:DlgRestart.Checked
        restartDelaySec = [int]$script:DlgDelay.Value
        dependencies = @($deps)
        addedAt = $addedAt
        updatedAt = (Get-Date).ToString('o')
    }

    $script:DlgForm.Tag = [pscustomobject]@{
        Service = $newService
        StartAfter = [bool]$script:DlgStart.Checked
    }
    $script:DlgForm.DialogResult = [System.Windows.Forms.DialogResult]::OK
    $script:DlgForm.Close()
}

function Show-ServiceDialog {
    param($Service, [bool]$IsEdit)

    $dlg = New-Object System.Windows.Forms.Form
    $dlg.Text = 'Tambah Service'
    if ($IsEdit) { $dlg.Text = 'Edit Service' }
    $dlg.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::FixedDialog
    $dlg.MaximizeBox = $false
    $dlg.MinimizeBox = $false
    $dlg.ShowInTaskbar = $false
    $dlg.StartPosition = [System.Windows.Forms.FormStartPosition]::CenterParent
    $dlg.ClientSize = New-UiSize 720 540
    $dlg.Font = New-Object System.Drawing.Font -ArgumentList 'Segoe UI', 9
    $dlg.Tag = $null

    $script:DlgForm = $dlg
    $script:DlgService = $Service
    $script:DlgIsNew = -not $IsEdit

    function New-DialogLabel {
        param([string]$Text, [int]$Y)
        $label = New-Object System.Windows.Forms.Label
        $label.Text = $Text
        $label.Location = New-UiPoint 16 ($Y + 4)
        $label.Size = New-UiSize 140 20
        $label.AutoSize = $false
        $dlg.Controls.Add($label)
        return $label
    }

    function New-DialogTextBox {
        param([int]$Y, [int]$Width = 465)
        $box = New-Object System.Windows.Forms.TextBox
        $box.Location = New-UiPoint 165 $Y
        $box.Size = New-UiSize $Width 25
        $dlg.Controls.Add($box)
        return $box
    }

    function New-BrowseButton {
        param([int]$Y)
        $button = New-Object System.Windows.Forms.Button
        $button.Text = '...'
        $button.Location = New-UiPoint 638 $Y
        $button.Size = New-UiSize 66 25
        $dlg.Controls.Add($button)
        return $button
    }

    [void](New-DialogLabel 'Jenis service' 16)
    $cmbType = New-Object System.Windows.Forms.ComboBox
    $cmbType.Location = New-UiPoint 165 16
    $cmbType.Size = New-UiSize 300 25
    $cmbType.DropDownStyle = [System.Windows.Forms.ComboBoxStyle]::DropDownList
    [void]$cmbType.Items.Add('NSSM (aplikasi umum)')
    [void]$cmbType.Items.Add('Apache (native)')
    [void]$cmbType.Items.Add('PostgreSQL (native)')
    $cmbType.SelectedIndex = 0
    $dlg.Controls.Add($cmbType)
    $script:DlgType = $cmbType

    [void](New-DialogLabel 'Nama service' 48)
    $txtName = New-DialogTextBox 48
    $script:DlgName = $txtName

    [void](New-DialogLabel 'Display name' 80)
    $txtDisplay = New-DialogTextBox 80
    $script:DlgDisplay = $txtDisplay

    $script:DlgLblExe = New-DialogLabel 'Application' 112
    $txtExe = New-DialogTextBox 112
    $script:DlgExe = $txtExe
    $btnExe = New-BrowseButton 112

    $script:DlgLblArgs = New-DialogLabel 'Arguments' 144
    $txtArgs = New-DialogTextBox 144
    $script:DlgArgs = $txtArgs

    $script:DlgLblWork = New-DialogLabel 'Working directory' 176
    $txtWork = New-DialogTextBox 176
    $script:DlgWork = $txtWork
    $btnWork = New-BrowseButton 176
    $script:DlgWorkBrowse = $btnWork

    $script:DlgLblApacheRoot = New-DialogLabel 'Apache root' 144
    $txtApacheRoot = New-DialogTextBox 144
    $script:DlgApacheRoot = $txtApacheRoot
    $btnApacheRoot = New-BrowseButton 144
    $script:DlgApacheRootBrowse = $btnApacheRoot

    $script:DlgLblApacheConfig = New-DialogLabel 'Config file' 176
    $txtApacheConfig = New-DialogTextBox 176
    $script:DlgApacheConfig = $txtApacheConfig
    $btnApacheConfig = New-BrowseButton 176
    $script:DlgApacheConfigBrowse = $btnApacheConfig

    $script:DlgLblPgData = New-DialogLabel 'PGDATA' 144
    $txtPgData = New-DialogTextBox 144
    $script:DlgPgData = $txtPgData
    $btnPgData = New-BrowseButton 144
    $script:DlgPgDataBrowse = $btnPgData

    $script:DlgLblPgAccount = New-DialogLabel 'Service account' 176
    $txtPgAccount = New-DialogTextBox 176
    $script:DlgPgAccount = $txtPgAccount

    [void](New-DialogLabel 'Stdout log' 208)
    $txtOut = New-DialogTextBox 208
    $script:DlgStdout = $txtOut
    $btnOut = New-BrowseButton 208
    $script:DlgOutBrowse = $btnOut

    [void](New-DialogLabel 'Stderr log' 240)
    $txtErr = New-DialogTextBox 240
    $script:DlgStderr = $txtErr
    $btnErr = New-BrowseButton 240
    $script:DlgErrBrowse = $btnErr

    $chkSame = New-Object System.Windows.Forms.CheckBox
    $chkSame.Text = 'Stderr sama dengan stdout'
    $chkSame.Location = New-UiPoint 165 272
    $chkSame.Size = New-UiSize 300 24
    $chkSame.Checked = $true
    $dlg.Controls.Add($chkSame)
    $script:DlgSame = $chkSame

    [void](New-DialogLabel 'Startup' 300)
    $cmbStartup = New-Object System.Windows.Forms.ComboBox
    $cmbStartup.Location = New-UiPoint 165 300
    $cmbStartup.Size = New-UiSize 220 25
    $cmbStartup.DropDownStyle = [System.Windows.Forms.ComboBoxStyle]::DropDownList
    [void]$cmbStartup.Items.Add('Automatic')
    [void]$cmbStartup.Items.Add('Automatic (Delayed)')
    [void]$cmbStartup.Items.Add('Manual')
    $cmbStartup.SelectedIndex = 0
    $dlg.Controls.Add($cmbStartup)
    $script:DlgStartup = $cmbStartup

    $chkRestart = New-Object System.Windows.Forms.CheckBox
    $chkRestart.Text = 'Restart jika service berhenti'
    $chkRestart.Location = New-UiPoint 165 332
    $chkRestart.Size = New-UiSize 200 24
    $chkRestart.Checked = $true
    $dlg.Controls.Add($chkRestart)
    $script:DlgRestart = $chkRestart

    $numDelay = New-Object System.Windows.Forms.NumericUpDown
    $numDelay.Location = New-UiPoint 370 330
    $numDelay.Size = New-UiSize 65 25
    $numDelay.Minimum = 1
    $numDelay.Maximum = 600
    $numDelay.Value = 5
    $dlg.Controls.Add($numDelay)
    $script:DlgDelay = $numDelay

    $lblDelay = New-Object System.Windows.Forms.Label
    $lblDelay.Text = 'detik (1-600)'
    $lblDelay.Location = New-UiPoint 442 334
    $lblDelay.Size = New-UiSize 120 20
    $dlg.Controls.Add($lblDelay)

    [void](New-DialogLabel 'Dependencies' 366)
    $txtDeps = New-DialogTextBox 366
    $script:DlgDeps = $txtDeps

    $chkStart = New-Object System.Windows.Forms.CheckBox
    $chkStart.Text = 'Start service setelah disimpan'
    $chkStart.Location = New-UiPoint 165 398
    $chkStart.Size = New-UiSize 300 24
    $chkStart.Checked = $true
    $dlg.Controls.Add($chkStart)
    $script:DlgStart = $chkStart

    $hint = New-Object System.Windows.Forms.Label
    $hint.Text = 'Dependencies dipisahkan spasi atau koma. Untuk file .cmd/.bat: pilih C:\Windows\System32\cmd.exe sebagai Application, lalu isi Arguments: /c "C:\path\file.cmd".'
    $hint.Location = New-UiPoint 16 430
    $hint.Size = New-UiSize 688 46
    $hint.ForeColor = [System.Drawing.Color]::DimGray
    $dlg.Controls.Add($hint)
    $script:DlgHint = $hint

    $btnSave = New-Object System.Windows.Forms.Button
    $btnSave.Text = 'Simpan'
    $btnSave.Location = New-UiPoint 545 490
    $btnSave.Size = New-UiSize 75 32
    $dlg.Controls.Add($btnSave)

    $btnCancel = New-Object System.Windows.Forms.Button
    $btnCancel.Text = 'Batal'
    $btnCancel.Location = New-UiPoint 628 490
    $btnCancel.Size = New-UiSize 76 32
    $btnCancel.DialogResult = [System.Windows.Forms.DialogResult]::Cancel
    $dlg.Controls.Add($btnCancel)

    $dlg.AcceptButton = $btnSave
    $dlg.CancelButton = $btnCancel

    $btnExe.Add_Click({ Select-ServiceExe })
    $btnWork.Add_Click({ Select-ServiceWorkDir })
    $btnApacheRoot.Add_Click({ Select-ApacheRoot })
    $btnApacheConfig.Add_Click({ Select-ApacheConfig })
    $btnPgData.Add_Click({ Select-PostgreSqlData })
    $btnOut.Add_Click({ Select-ServiceStdoutLog })
    $btnErr.Add_Click({ Select-ServiceStderrLog })
    $chkSame.Add_CheckedChanged({ Update-ServiceLogFields })
    $txtOut.Add_TextChanged({ Update-ServiceLogFields })
    $btnSave.Add_Click({ Save-ServiceDialog })

    $typeIndex = 0
    if ($null -ne $Service) {
        if ($Service.serviceType -eq 'apache') { $typeIndex = 1 }
        elseif ($Service.serviceType -eq 'postgresql') { $typeIndex = 2 }
    }
    $cmbType.SelectedIndex = $typeIndex

    if ($null -ne $Service) {
        $txtName.Text = [string]$Service.name
        $txtDisplay.Text = [string]$Service.displayName
        $txtExe.Text = [string]$Service.executable
        $txtArgs.Text = [string]$Service.arguments
        $txtWork.Text = [string]$Service.workingDirectory
        $txtApacheRoot.Text = [string]$Service.apacheRoot
        $txtApacheConfig.Text = [string]$Service.apacheConfig
        $txtPgData.Text = [string]$Service.pgData
        $txtPgAccount.Text = [string]$Service.pgAccount
        $txtOut.Text = [string]$Service.stdoutLog
        $txtErr.Text = [string]$Service.stderrLog
        $txtDeps.Text = (@($Service.dependencies) -join ', ')
        $chkRestart.Checked = [bool]$Service.restartOnCrash
        $delayValue = [int]$Service.restartDelaySec
        if ($delayValue -lt 1) { $delayValue = 1 }
        if ($delayValue -gt 600) { $delayValue = 600 }
        $numDelay.Value = $delayValue

        if ($Service.startup -eq 'delayed') { $cmbStartup.SelectedIndex = 1 }
        elseif ($Service.startup -eq 'manual') { $cmbStartup.SelectedIndex = 2 }
        else { $cmbStartup.SelectedIndex = 0 }

        if ($typeIndex -eq 1) {
            if ([string]::IsNullOrWhiteSpace($txtApacheRoot.Text)) { $txtApacheRoot.Text = Get-ApacheRootFromExe $txtExe.Text }
            if ([string]::IsNullOrWhiteSpace($txtApacheConfig.Text)) { $txtApacheConfig.Text = 'conf/httpd.conf' }
        } elseif ($typeIndex -eq 2) {
            if ([string]::IsNullOrWhiteSpace($txtPgAccount.Text)) { $txtPgAccount.Text = 'NT AUTHORITY\NetworkService' }
        }

        $sameLog = [string]::IsNullOrEmpty([string]$Service.stderrLog) -or [string]::Equals([string]$Service.stderrLog, [string]$Service.stdoutLog, [System.StringComparison]::OrdinalIgnoreCase)
        $chkSame.Checked = $sameLog
    } else {
        $chkSame.Checked = $true
        $chkStart.Checked = $true
    }

    $txtName.Enabled = -not $IsEdit
    $cmbType.Enabled = -not $IsEdit
    $chkStart.Visible = -not $IsEdit
    if ($IsEdit) { $chkStart.Checked = $false }

    Update-ServiceTypeFields
    Update-ServiceLogFields
    $cmbType.Add_SelectedIndexChanged({ Update-ServiceTypeFields -ApplyDefaults })

    $dialogResult = $dlg.ShowDialog($script:Form)
    $tag = $dlg.Tag
    $dlg.Dispose()

    $script:DlgForm = $null
    $script:DlgType = $null
    $script:DlgName = $null
    $script:DlgDisplay = $null
    $script:DlgExe = $null
    $script:DlgLblExe = $null
    $script:DlgLblArgs = $null
    $script:DlgLblWork = $null
    $script:DlgLblApacheRoot = $null
    $script:DlgLblApacheConfig = $null
    $script:DlgLblPgData = $null
    $script:DlgLblPgAccount = $null
    $script:DlgArgs = $null
    $script:DlgWork = $null
    $script:DlgApacheRoot = $null
    $script:DlgApacheConfig = $null
    $script:DlgPgData = $null
    $script:DlgPgAccount = $null
    $script:DlgApacheRootBrowse = $null
    $script:DlgApacheConfigBrowse = $null
    $script:DlgPgDataBrowse = $null
    $script:DlgWorkBrowse = $null
    $script:DlgHint = $null
    $script:DlgStdout = $null
    $script:DlgStderr = $null
    $script:DlgSame = $null
    $script:DlgStartup = $null
    $script:DlgRestart = $null
    $script:DlgDelay = $null
    $script:DlgDeps = $null
    $script:DlgStart = $null
    $script:DlgOutBrowse = $null
    $script:DlgErrBrowse = $null
    $script:DlgService = $null

    if ($dialogResult -eq [System.Windows.Forms.DialogResult]::OK) { return $tag }
    return $null
}

function Confirm-ImportDialog {
    if ($null -eq $script:ImportForm -or $null -eq $script:ImportList) { return }

    $selected = @()
    for ($i = 0; $i -lt $script:ImportList.Items.Count; $i++) {
        if ($script:ImportList.GetItemChecked($i)) {
            $selected += $script:ImportList.Items[$i].Service
        }
    }

    if ($selected.Count -eq 0) {
        [System.Windows.Forms.MessageBox]::Show($script:ImportForm, 'Centang minimal satu service untuk diimpor.', 'Import Service', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Information) | Out-Null
        return
    }

    $script:ImportForm.Tag = $selected
    $script:ImportForm.DialogResult = [System.Windows.Forms.DialogResult]::OK
    $script:ImportForm.Close()
}

function Show-ImportDialog {
    $existingNames = @{}
    foreach ($svc in $script:Services) { $existingNames[[string]$svc.name] = $true }

    $candidates = @(Get-ManagedServiceCandidates)
    $newCandidates = @()
    foreach ($candidate in $candidates) {
        if ($existingNames.ContainsKey([string]$candidate.name)) { continue }
        $newCandidates += $candidate
    }

    if ($newCandidates.Count -eq 0) {
        Show-Info 'Tidak ada service baru yang bisa diimpor. Semua service yang terdeteksi sudah ada di daftar.'
        return @()
    }

    $form = New-Object System.Windows.Forms.Form
    $form.Text = 'Import Service'
    $form.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::FixedDialog
    $form.MaximizeBox = $false
    $form.MinimizeBox = $false
    $form.ShowInTaskbar = $false
    $form.StartPosition = [System.Windows.Forms.FormStartPosition]::CenterParent
    $form.ClientSize = New-UiSize 640 395
    $form.Font = New-Object System.Drawing.Font -ArgumentList 'Segoe UI', 9
    $form.Tag = $null
    $script:ImportForm = $form

    $label = New-Object System.Windows.Forms.Label
    $label.Text = 'Service NSSM/native berikut belum ada di daftar aplikasi. Centang service yang ingin diimpor.'
    $label.Location = New-UiPoint 12 12
    $label.Size = New-UiSize 616 34
    $form.Controls.Add($label)

    $list = New-Object System.Windows.Forms.CheckedListBox
    $list.Location = New-UiPoint 12 50
    $list.Size = New-UiSize 616 290
    $list.CheckOnClick = $true
    $list.IntegralHeight = $false
    $list.DisplayMember = 'Text'
    $form.Controls.Add($list)
    $script:ImportList = $list

    foreach ($candidate in $newCandidates) {
        $item = [pscustomobject]@{
            Text = '{0}  [{1}]  - {2}' -f $candidate.displayName, $candidate.name, (Get-ServiceTypeLabel $candidate.serviceType)
            Service = $candidate
        }
        $index = $list.Items.Add($item)
        $list.SetItemChecked($index, $true)
    }

    $btnImport = New-Object System.Windows.Forms.Button
    $btnImport.Text = 'Import'
    $btnImport.Location = New-UiPoint 450 352
    $btnImport.Size = New-UiSize 90 30
    $form.Controls.Add($btnImport)

    $btnCancel = New-Object System.Windows.Forms.Button
    $btnCancel.Text = 'Batal'
    $btnCancel.Location = New-UiPoint 548 352
    $btnCancel.Size = New-UiSize 80 30
    $btnCancel.DialogResult = [System.Windows.Forms.DialogResult]::Cancel
    $form.Controls.Add($btnCancel)

    $form.AcceptButton = $btnImport
    $form.CancelButton = $btnCancel
    $btnImport.Add_Click({ Confirm-ImportDialog })

    $dialogResult = $form.ShowDialog($script:Form)
    $tag = $form.Tag
    $form.Dispose()

    $script:ImportForm = $null
    $script:ImportList = $null

    if ($dialogResult -eq [System.Windows.Forms.DialogResult]::OK -and $null -ne $tag) { return $tag }
    return @()
}

function Add-ServiceUi {
    Invoke-UiAction {
        $result = Show-ServiceDialog -Service $null -IsEdit $false
        if ($null -eq $result) { return }

        $svc = $result.Service
        [void]$script:Services.Add($svc)

        try {
            Save-Config
        } catch {
            [void]$script:Services.Remove($svc)
            throw
        }

        Refresh-Grid

        try {
            Install-ManagedService -Svc $svc -StartAfter:([bool]$result.StartAfter)
        } catch {
            Refresh-Grid
            if (Test-ServiceExists $svc.name) {
                Show-Error ("Config '$($svc.name)' sudah disimpan dan service terpasang, tetapi proses install/start gagal.`r`n`r`n" + $_.Exception.Message + "`r`n`r`nCek status service; gunakan tombol Start atau Install ulang jika perlu.")
            } else {
                Show-Error ("Config '$($svc.name)' sudah disimpan, tetapi service gagal dipasang.`r`n`r`n" + $_.Exception.Message + "`r`n`r`nPerbaiki data lalu klik tombol Install.")
            }
            return
        }

        Refresh-Grid
        if ([bool]$result.StartAfter) {
            Set-AppStatus "Service '$($svc.name)' dipasang dan dijalankan."
        } else {
            Set-AppStatus "Service '$($svc.name)' dipasang."
        }
    }
}

function Edit-ServiceUi {
    $svc = Get-SelectedService
    if ($null -eq $svc) {
        Show-Info 'Pilih service yang akan diedit.'
        return
    }

    Invoke-UiAction {
        $result = Show-ServiceDialog -Service $svc -IsEdit $true
        if ($null -eq $result) { return }

        $updated = $result.Service
        $index = -1
        for ($i = 0; $i -lt $script:Services.Count; $i++) {
            if ([string]::Equals([string]$script:Services[$i].name, [string]$updated.name, [System.StringComparison]::OrdinalIgnoreCase)) {
                $index = $i
                break
            }
        }

        if ($index -ge 0) {
            $script:Services[$index] = $updated
        } else {
            [void]$script:Services.Add($updated)
        }

        Save-Config
        Refresh-Grid

        if (Test-ServiceExists $updated.name) {
            $wasRunning = (Get-ServiceStatus $updated.name) -eq 'Running'
            try {
                Apply-ServiceConfig $updated
            } catch {
                Refresh-Grid
                Show-Error ("Config tersimpan, tetapi penerapan ke service gagal.`r`n`r`n" + $_.Exception.Message)
                return
            }

            if ($wasRunning) {
                $answer = [System.Windows.Forms.MessageBox]::Show($script:Form, "Service '$($updated.name)' sedang berjalan. Restart sekarang agar perubahan berlaku?", 'Restart Service', [System.Windows.Forms.MessageBoxButtons]::YesNo, [System.Windows.Forms.MessageBoxIcon]::Question)
                if ($answer -eq [System.Windows.Forms.DialogResult]::Yes) {
                    Restart-ManagedService $updated.name
                }
            }
            Set-AppStatus "Config dan service '$($updated.name)' diperbarui."
        } else {
            Set-AppStatus "Config '$($updated.name)' disimpan (service belum terpasang)."
            Show-Info "Config '$($updated.name)' disimpan. Service belum terpasang - gunakan tombol Install untuk memasangnya."
        }

        Refresh-Grid
    }
}

function Delete-ServiceUi {
    $svc = Get-SelectedService
    if ($null -eq $svc) {
        Show-Info 'Pilih service yang akan dihapus.'
        return
    }

    Invoke-UiAction {
        $installed = Test-ServiceExists $svc.name

        if ($installed) {
            $nativeNote = ''
            if (Test-NativeServiceType (Get-ManagedServiceType $svc.name)) {
                $nativeNote = "`r`n`r`nService native akan di-uninstall. Data/config aplikasi tidak dihapus."
            }
            $answer = [System.Windows.Forms.MessageBox]::Show($script:Form, "Hapus service '$($svc.name)'?`r`n`r`nYa  = hapus dari Windows dan config.`r`nTidak = hapus dari config saja (service tetap terpasang).`r`nBatal = batal.$nativeNote", 'Hapus Service', [System.Windows.Forms.MessageBoxButtons]::YesNoCancel, [System.Windows.Forms.MessageBoxIcon]::Question)
            if ($answer -eq [System.Windows.Forms.DialogResult]::Cancel) { return }
            if ($answer -eq [System.Windows.Forms.DialogResult]::Yes) {
                Remove-ManagedServiceFromWindows $svc.name
            }
        } else {
            $answer = [System.Windows.Forms.MessageBox]::Show($script:Form, "Service '$($svc.name)' belum terpasang. Hapus dari daftar config?", 'Hapus Service', [System.Windows.Forms.MessageBoxButtons]::YesNo, [System.Windows.Forms.MessageBoxIcon]::Question)
            if ($answer -ne [System.Windows.Forms.DialogResult]::Yes) { return }
        }

        Remove-ServiceFromList $svc.name
        Save-Config
        Refresh-Grid
        Set-AppStatus "Service '$($svc.name)' dihapus dari daftar."
    }
}

function Install-ServiceUi {
    $svc = Get-SelectedService
    if ($null -eq $svc) {
        Show-Info 'Pilih service yang akan dipasang.'
        return
    }

    Invoke-UiAction {
        if (Test-ServiceExists $svc.name) {
            Show-Info "Service '$($svc.name)' sudah terpasang."
            return
        }

        Install-ManagedService -Svc $svc
        Refresh-Grid
        Set-AppStatus "Service '$($svc.name)' dipasang."

        $answer = [System.Windows.Forms.MessageBox]::Show($script:Form, "Service '$($svc.name)' berhasil dipasang. Jalankan sekarang?", 'Start Service', [System.Windows.Forms.MessageBoxButtons]::YesNo, [System.Windows.Forms.MessageBoxIcon]::Question)
        if ($answer -eq [System.Windows.Forms.DialogResult]::Yes) {
            Start-ManagedService $svc.name
            Refresh-Grid
            Set-AppStatus "Service '$($svc.name)' berjalan."
        }
    }
}

function Start-ServiceUi {
    $svc = Get-SelectedService
    if ($null -eq $svc) {
        Show-Info 'Pilih service yang akan dijalankan.'
        return
    }

    Invoke-UiAction {
        if (-not (Test-ServiceExists $svc.name)) {
            Show-Info "Service '$($svc.name)' belum terpasang. Klik Install terlebih dahulu."
            return
        }
        Start-ManagedService $svc.name
        Refresh-Grid
        Set-AppStatus "Service '$($svc.name)' berjalan."
    }
}

function Stop-ServiceUi {
    $svc = Get-SelectedService
    if ($null -eq $svc) {
        Show-Info 'Pilih service yang akan dihentikan.'
        return
    }

    Invoke-UiAction {
        if (-not (Test-ServiceExists $svc.name)) {
            Show-Info "Service '$($svc.name)' belum terpasang."
            return
        }
        Stop-ManagedService $svc.name
        Refresh-Grid
        Set-AppStatus "Service '$($svc.name)' berhenti."
    }
}

function Restart-ServiceUi {
    $svc = Get-SelectedService
    if ($null -eq $svc) {
        Show-Info 'Pilih service yang akan di-restart.'
        return
    }

    Invoke-UiAction {
        if (-not (Test-ServiceExists $svc.name)) {
            Show-Info "Service '$($svc.name)' belum terpasang."
            return
        }
        Restart-ManagedService $svc.name
        Refresh-Grid
        Set-AppStatus "Service '$($svc.name)' di-restart."
    }
}

function Start-AllUi {
    Invoke-UiAction {
        $targets = @(Get-ManagedServicesInStartOrder)
        if ($targets.Count -eq 0) {
            Show-Info 'Tidak ada service yang terpasang.'
            return
        }

        $failed = New-Object System.Collections.ArrayList
        foreach ($svc in $targets) {
            try {
                Start-ManagedService $svc.name
            } catch {
                [void]$failed.Add(('{0}: {1}' -f $svc.name, $_.Exception.Message))
            }
        }

        Refresh-Grid
        if ($failed.Count -gt 0) {
            Show-Error ("Beberapa service gagal dijalankan:`r`n`r`n" + (($failed.ToArray()) -join "`r`n"))
        } else {
            Set-AppStatus 'Semua service terpasang berhasil dijalankan.'
        }
    }
}

function Stop-AllUi {
    Invoke-UiAction {
        $targets = @(Get-ManagedServicesInStopOrder)
        if ($targets.Count -eq 0) {
            Show-Info 'Tidak ada service yang terpasang.'
            return
        }

        $failed = New-Object System.Collections.ArrayList
        foreach ($svc in $targets) {
            try {
                Stop-ManagedService $svc.name
            } catch {
                [void]$failed.Add(('{0}: {1}' -f $svc.name, $_.Exception.Message))
            }
        }

        Refresh-Grid
        if ($failed.Count -gt 0) {
            Show-Error ("Beberapa service gagal dihentikan:`r`n`r`n" + (($failed.ToArray()) -join "`r`n"))
        } else {
            Set-AppStatus 'Semua service terpasang berhasil dihentikan.'
        }
    }
}

function Restart-AllUi {
    Invoke-UiAction {
        $targets = @(Get-ManagedServicesInStartOrder)
        if ($targets.Count -eq 0) {
            Show-Info 'Tidak ada service yang terpasang.'
            return
        }

        $failed = New-Object System.Collections.ArrayList
        foreach ($svc in @(Get-ManagedServicesInStopOrder)) {
            try {
                Stop-ManagedService $svc.name
            } catch {
                [void]$failed.Add(('stop {0}: {1}' -f $svc.name, $_.Exception.Message))
            }
        }

        foreach ($svc in @(Get-ManagedServicesInStartOrder)) {
            try {
                Start-ManagedService $svc.name
            } catch {
                [void]$failed.Add(('start {0}: {1}' -f $svc.name, $_.Exception.Message))
            }
        }

        Refresh-Grid
        if ($failed.Count -gt 0) {
            Show-Error ("Beberapa service gagal di-restart:`r`n`r`n" + (($failed.ToArray()) -join "`r`n"))
        } else {
            Set-AppStatus 'Semua service terpasang berhasil di-restart.'
        }
    }
}

function Import-ServicesUi {
    Invoke-UiAction {
        $selected = @(Show-ImportDialog)
        if ($selected.Count -eq 0) { return }

        $added = 0
        foreach ($candidate in $selected) {
            $exists = $false
            foreach ($svc in $script:Services) {
                if ([string]::Equals([string]$svc.name, [string]$candidate.name, [System.StringComparison]::OrdinalIgnoreCase)) {
                    $exists = $true
                    break
                }
            }
            if (-not $exists) {
                [void]$script:Services.Add($candidate)
                $added++
            }
        }

        if ($added -gt 0) {
            Save-Config
            Refresh-Grid
            Set-AppStatus "$added service diimpor."
            Show-Info "$added service ditambahkan ke daftar. Service yang belum terpasang akan tampil 'Not Installed'."
        } else {
            Show-Info 'Tidak ada service baru yang diimpor.'
        }
    }
}

function Export-ConfigUi {
    if ($null -eq $script:Form) { return }

    $dialog = New-Object System.Windows.Forms.SaveFileDialog
    $dialog.Title = 'Backup config service'
    $dialog.Filter = 'JSON (*.json)|*.json|Semua file (*.*)|*.*'
    $dialog.DefaultExt = 'json'
    $dialog.AddExtension = $true
    $dialog.OverwritePrompt = $true
    $dialog.FileName = ('services-export-{0}.json' -f (Get-Date -Format 'yyyyMMdd-HHmm'))
    $dialog.InitialDirectory = $script:AppDir

    if ($dialog.ShowDialog($script:Form) -ne [System.Windows.Forms.DialogResult]::OK) { return }

    try {
        $json = Get-ConfigJson
        $utf8 = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($dialog.FileName, $json, $utf8)
        Write-AppLog "Export config: $($dialog.FileName)"
        Show-Info "Backup config disimpan:`r`n$($dialog.FileName)"
    } catch {
        Show-Error "Gagal menyimpan backup config:`r`n$($_.Exception.Message)"
    }
}

function Import-ConfigUi {
    if ($null -eq $script:Form) { return }

    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Title = 'Restore config service'
    $dialog.Filter = 'JSON (*.json)|*.json|Semua file (*.*)|*.*'
    $dialog.InitialDirectory = $script:AppDir

    if ($dialog.ShowDialog($script:Form) -ne [System.Windows.Forms.DialogResult]::OK) { return }

    try {
        $raw = Get-Content -LiteralPath $dialog.FileName -Raw -Encoding UTF8
        $data = $raw | ConvertFrom-Json

        $items = @()
        if ($data -is [array]) {
            $items = @($data)
        } elseif ($null -ne $data -and $null -ne $data.PSObject.Properties['services']) {
            $items = @($data.services)
        } else {
            throw 'Format JSON tidak dikenali. File harus berisi object dengan properti services.'
        }

        $imported = @()
        foreach ($item in $items) {
            if ($null -eq $item) { continue }
            $cfg = ConvertTo-ServiceConfig $item
            if (-not [string]::IsNullOrWhiteSpace($cfg.name)) { $imported += $cfg }
        }
        if ($imported.Count -eq 0) { throw 'Tidak ada service valid di file tersebut.' }

        $answer = [System.Windows.Forms.MessageBox]::Show($script:Form, "Ditemukan $($imported.Count) service di file.`r`n`r`nYa = ganti seluruh daftar config.`r`nTidak = gabungkan ke daftar sekarang.`r`nBatal = batal.`r`n`r`nCatatan: restore hanya mengubah config, tidak menghapus service yang sudah terpasang di Windows.", 'Restore Config', [System.Windows.Forms.MessageBoxButtons]::YesNoCancel, [System.Windows.Forms.MessageBoxIcon]::Question)
        if ($answer -eq [System.Windows.Forms.DialogResult]::Cancel) { return }

        if ($answer -eq [System.Windows.Forms.DialogResult]::Yes) {
            $script:Services = New-Object System.Collections.ArrayList
            foreach ($cfg in $imported) { [void]$script:Services.Add($cfg) }
            $changed = $imported.Count
        } else {
            $changed = 0
            foreach ($cfg in $imported) {
                $exists = $false
                foreach ($svc in $script:Services) {
                    if ([string]::Equals([string]$svc.name, [string]$cfg.name, [System.StringComparison]::OrdinalIgnoreCase)) {
                        $exists = $true
                        break
                    }
                }
                if (-not $exists) {
                    [void]$script:Services.Add($cfg)
                    $changed++
                }
            }
        }

        Save-Config
        Refresh-Grid
        Set-AppStatus "Config direstore ($changed service dimuat)."
        Show-Info "Restore selesai. $changed service dimuat. Service yang belum terpasang akan tampil 'Not Installed'."
    } catch {
        Write-AppLog "Restore config gagal: $($_.Exception.Message)"
        Show-Error "Gagal restore config:`r`n$($_.Exception.Message)"
    }
}

function Open-SelectedLogUi {
    $svc = Get-SelectedService
    if ($null -eq $svc) {
        Show-Info 'Pilih service terlebih dahulu.'
        return
    }

    $path = [string]$svc.stdoutLog
    if ([string]::IsNullOrWhiteSpace($path)) { $path = [string]$svc.stderrLog }
    if ([string]::IsNullOrWhiteSpace($path)) {
        Show-Info 'Service ini belum punya path log. Klik Edit untuk mengaturnya.'
        return
    }
    if (-not (Test-Path -LiteralPath $path)) {
        Show-Info "File log belum ada:`r`n$path"
        return
    }

    Start-Process -FilePath notepad.exe -ArgumentList ('"{0}"' -f $path)
}

function Open-AppFolder {
    Start-Process -FilePath explorer.exe -ArgumentList ('"{0}"' -f $script:AppDir)
}

function Open-AppLog {
    if (-not (Test-Path -LiteralPath $script:AppLogPath)) {
        New-Item -ItemType File -Path $script:AppLogPath -Force | Out-Null
    }
    Start-Process -FilePath notepad.exe -ArgumentList ('"{0}"' -f $script:AppLogPath)
}

function Show-About {
    $message = "Service Manager 1.0`r`n`r`nMengelola service Windows melalui NSSM serta Apache/PostgreSQL native.`r`n`r`nConfig : $($script:ConfigPath)`r`nNSSM   : $($script:NssmPath)`r`nBackup : $($script:BackupDir)`r`nLog    : $($script:AppLogPath)`r`n`r`nAplikasi harus dijalankan sebagai Administrator."
    Show-Info $message
}

if ($Diagnose) {
    Write-Output 'Service Manager - diagnosa'
    Write-Output ('AppDir        : {0}' -f $script:AppDir)
    Write-Output ('Config        : {0} (ada: {1})' -f $script:ConfigPath, (Test-Path -LiteralPath $script:ConfigPath))
    Write-Output ('NSSM          : {0} (ada: {1})' -f $script:NssmPath, (Test-Path -LiteralPath $script:NssmPath))
    Write-Output ('Administrator : {0}' -f (Test-Admin))
    Write-Output 'Service yang terdeteksi:'
    try {
        $candidates = @(Get-ManagedServiceCandidates)
        if ($candidates.Count -eq 0) {
            Write-Output '  (tidak ada)'
        } else {
            foreach ($candidate in $candidates) {
                Write-Output ('  - [{0}] {1} [{2}] -> {3} (startup: {4})' -f (Get-ServiceTypeLabel $candidate.serviceType), $candidate.displayName, $candidate.name, $candidate.executable, $candidate.startup)
            }
        }
    } catch {
        Write-Output ('  ERROR: {0}' -f $_.Exception.Message)
    }
    $configServices = Read-Config
    Write-Output ('Service di config: {0}' -f $configServices.Count)
    exit 0
}

if (-not (Test-Admin)) {
    try {
        $elevateArgs = '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "{0}"' -f $PSCommandPath
        Start-Process -FilePath "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -ArgumentList $elevateArgs -Verb RunAs -ErrorAction Stop
    } catch {
        [System.Windows.Forms.MessageBox]::Show("Aplikasi membutuhkan hak Administrator.`r`n`r`n$($_.Exception.Message)", 'Service Manager', [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Error) | Out-Null
    }
    exit
}

$script:Services = Read-Config

$form = New-Object System.Windows.Forms.Form
$script:Form = $form
$form.Text = 'Service Manager - NSSM & Native'
$form.StartPosition = [System.Windows.Forms.FormStartPosition]::CenterScreen
$form.ClientSize = New-UiSize 1180 700
$form.MinimumSize = New-UiSize 920 560
$form.Font = New-Object System.Drawing.Font -ArgumentList 'Segoe UI', 9
$form.Icon = [System.Drawing.SystemIcons]::Application

$script:StatusFont = New-Object System.Drawing.Font -ArgumentList 'Segoe UI', 9, ([System.Drawing.FontStyle]::Bold)

$mainMenu = New-Object System.Windows.Forms.MenuStrip
$mainMenu.Dock = [System.Windows.Forms.DockStyle]::Fill
$form.MainMenuStrip = $mainMenu

function Add-MenuItem {
    param($Parent, [string]$Text, [scriptblock]$OnClick)
    $item = New-Object System.Windows.Forms.ToolStripMenuItem
    $item.Text = $Text
    if ($null -ne $OnClick) { $item.Add_Click($OnClick) }
    [void]$Parent.DropDownItems.Add($item)
    return $item
}

function Add-MenuSeparator {
    param($Parent)
    [void]$Parent.DropDownItems.Add((New-Object System.Windows.Forms.ToolStripSeparator))
}

$menuApp = New-Object System.Windows.Forms.ToolStripMenuItem
$menuApp.Text = 'Aplikasi'
[void]$mainMenu.Items.Add($menuApp)
[void](Add-MenuItem $menuApp 'Import Service...' { Import-ServicesUi })
[void](Add-MenuSeparator $menuApp)
[void](Add-MenuItem $menuApp 'Backup Config (Export)...' { Export-ConfigUi })
[void](Add-MenuItem $menuApp 'Restore Config (Import)...' { Import-ConfigUi })
[void](Add-MenuSeparator $menuApp)
[void](Add-MenuItem $menuApp 'Buka Folder Aplikasi' { Open-AppFolder })
[void](Add-MenuItem $menuApp 'Buka Log Aplikasi' { Open-AppLog })
[void](Add-MenuSeparator $menuApp)
[void](Add-MenuItem $menuApp 'Keluar' { $script:Form.Close() })

$menuService = New-Object System.Windows.Forms.ToolStripMenuItem
$menuService.Text = 'Service'
[void]$mainMenu.Items.Add($menuService)
[void](Add-MenuItem $menuService 'Tambah Service...' { Add-ServiceUi })
[void](Add-MenuItem $menuService 'Edit Service...' { Edit-ServiceUi })
[void](Add-MenuItem $menuService 'Hapus Service...' { Delete-ServiceUi })
[void](Add-MenuSeparator $menuService)
[void](Add-MenuItem $menuService 'Install Service' { Install-ServiceUi })
[void](Add-MenuSeparator $menuService)
[void](Add-MenuItem $menuService 'Start Service' { Start-ServiceUi })
[void](Add-MenuItem $menuService 'Stop Service' { Stop-ServiceUi })
[void](Add-MenuItem $menuService 'Restart Service' { Restart-ServiceUi })
[void](Add-MenuSeparator $menuService)
[void](Add-MenuItem $menuService 'Start Semua' { Start-AllUi })
[void](Add-MenuItem $menuService 'Stop Semua' { Stop-AllUi })
[void](Add-MenuItem $menuService 'Restart Semua' { Restart-AllUi })
[void](Add-MenuSeparator $menuService)
[void](Add-MenuItem $menuService 'Refresh Status' { Refresh-Grid; Set-AppStatus 'Status diperbarui.' })

$menuHelp = New-Object System.Windows.Forms.ToolStripMenuItem
$menuHelp.Text = 'Bantuan'
[void]$mainMenu.Items.Add($menuHelp)
[void](Add-MenuItem $menuHelp 'Tentang' { Show-About })

$toolStrip = New-Object System.Windows.Forms.ToolStrip
$toolStrip.Dock = [System.Windows.Forms.DockStyle]::Fill
$toolStrip.GripStyle = [System.Windows.Forms.ToolStripGripStyle]::Hidden
$toolStrip.Padding = New-Object System.Windows.Forms.Padding -ArgumentList @(4, 2, 4, 2)

function Add-ToolButton {
    param($ToolStrip, [string]$Text, [scriptblock]$OnClick, [string]$ToolTip = '')
    $button = New-Object System.Windows.Forms.ToolStripButton
    $button.Text = $Text
    $button.DisplayStyle = [System.Windows.Forms.ToolStripItemDisplayStyle]::Text
    $button.ToolTipText = $ToolTip
    $button.Padding = New-Object System.Windows.Forms.Padding -ArgumentList @(3, 0, 3, 0)
    if ($null -ne $OnClick) { $button.Add_Click($OnClick) }
    [void]$ToolStrip.Items.Add($button)
    return $button
}

[void](Add-ToolButton $toolStrip 'Tambah' { Add-ServiceUi } 'Tambah service baru')
[void](Add-ToolButton $toolStrip 'Edit' { Edit-ServiceUi } 'Edit service terpilih')
[void](Add-ToolButton $toolStrip 'Hapus' { Delete-ServiceUi } 'Hapus service terpilih')
[void](Add-ToolButton $toolStrip 'Install' { Install-ServiceUi } 'Pasang service dari config')
[void]$toolStrip.Items.Add((New-Object System.Windows.Forms.ToolStripSeparator))
[void](Add-ToolButton $toolStrip 'Import' { Import-ServicesUi } 'Import service NSSM/native yang sudah ada')
[void](Add-ToolButton $toolStrip 'Refresh' { Refresh-Grid; Set-AppStatus 'Status diperbarui.' } 'Refresh status')
[void]$toolStrip.Items.Add((New-Object System.Windows.Forms.ToolStripSeparator))
[void](Add-ToolButton $toolStrip 'Start' { Start-ServiceUi } 'Start service terpilih')
[void](Add-ToolButton $toolStrip 'Stop' { Stop-ServiceUi } 'Stop service terpilih')
[void](Add-ToolButton $toolStrip 'Restart' { Restart-ServiceUi } 'Restart service terpilih')
[void]$toolStrip.Items.Add((New-Object System.Windows.Forms.ToolStripSeparator))
[void](Add-ToolButton $toolStrip 'Start Semua' { Start-AllUi } 'Start semua service terpasang')
[void](Add-ToolButton $toolStrip 'Stop Semua' { Stop-AllUi } 'Stop semua service terpasang')
[void](Add-ToolButton $toolStrip 'Restart Semua' { Restart-AllUi } 'Restart semua service terpasang')
[void]$toolStrip.Items.Add((New-Object System.Windows.Forms.ToolStripSeparator))
[void](Add-ToolButton $toolStrip 'Backup' { Export-ConfigUi } 'Backup config ke file JSON')
[void](Add-ToolButton $toolStrip 'Restore' { Import-ConfigUi } 'Restore config dari file JSON')

$statusStrip = New-Object System.Windows.Forms.StatusStrip
$statusStrip.Dock = [System.Windows.Forms.DockStyle]::Fill

$lblStatus = New-Object System.Windows.Forms.ToolStripStatusLabel
$lblStatus.Text = 'Siap'
$lblStatus.Spring = $true
$lblStatus.TextAlign = [System.Drawing.ContentAlignment]::MiddleLeft
[void]$statusStrip.Items.Add($lblStatus)
$script:StatusLabel = $lblStatus

$lblCount = New-Object System.Windows.Forms.ToolStripStatusLabel
$lblCount.Text = '0 terdaftar'
$lblCount.BorderSides = [System.Windows.Forms.ToolStripStatusLabelBorderSides]::Left
[void]$statusStrip.Items.Add($lblCount)
$script:CountLabel = $lblCount

$split = New-Object System.Windows.Forms.SplitContainer
$script:Split = $split
$split.Dock = [System.Windows.Forms.DockStyle]::Fill
$split.Orientation = [System.Windows.Forms.Orientation]::Horizontal
$split.Panel1MinSize = 220
$split.Panel2MinSize = 130
$split.SplitterWidth = 6

$grid = New-Object System.Windows.Forms.DataGridView
$script:Grid = $grid
$grid.Dock = [System.Windows.Forms.DockStyle]::Fill
$grid.ReadOnly = $true
$grid.AllowUserToAddRows = $false
$grid.AllowUserToDeleteRows = $false
$grid.AllowUserToResizeRows = $false
$grid.AllowUserToOrderColumns = $false
$grid.MultiSelect = $false
$grid.SelectionMode = [System.Windows.Forms.DataGridViewSelectionMode]::FullRowSelect
$grid.RowHeadersVisible = $false
$grid.AutoSizeColumnsMode = [System.Windows.Forms.DataGridViewAutoSizeColumnsMode]::None
$grid.BackgroundColor = [System.Drawing.Color]::White
$grid.BorderStyle = [System.Windows.Forms.BorderStyle]::None
$grid.CellBorderStyle = [System.Windows.Forms.DataGridViewCellBorderStyle]::SingleHorizontal
$grid.EnableHeadersVisualStyles = $false
$grid.ColumnHeadersHeight = 30
$grid.ColumnHeadersDefaultCellStyle.BackColor = [System.Drawing.Color]::FromArgb(245, 246, 248)
$grid.ColumnHeadersDefaultCellStyle.Font = New-Object System.Drawing.Font -ArgumentList 'Segoe UI', 9, ([System.Drawing.FontStyle]::Bold)
$grid.ColumnHeadersDefaultCellStyle.Padding = New-Object System.Windows.Forms.Padding -ArgumentList @(4, 0, 4, 0)
$grid.AlternatingRowsDefaultCellStyle.BackColor = [System.Drawing.Color]::FromArgb(250, 251, 252)
$grid.RowTemplate.Height = 26
$grid.DefaultCellStyle.SelectionBackColor = [System.Drawing.Color]::FromArgb(204, 228, 247)
$grid.DefaultCellStyle.SelectionForeColor = [System.Drawing.Color]::Black
$grid.GridColor = [System.Drawing.Color]::FromArgb(225, 225, 225)

function New-GridColumn {
    param([string]$Name, [string]$Header, [int]$Width)
    $column = New-Object System.Windows.Forms.DataGridViewTextBoxColumn
    $column.Name = $Name
    $column.HeaderText = $Header
    $column.Width = $Width
    $column.ReadOnly = $true
    $column.SortMode = [System.Windows.Forms.DataGridViewColumnSortMode]::NotSortable
    return $column
}

$colStatus = New-GridColumn 'Status' 'Status' 110
$colName = New-GridColumn 'ServiceName' 'Nama Service' 150
$colDisplay = New-GridColumn 'DisplayName' 'Display Name' 170
$colType = New-GridColumn 'ServiceType' 'Jenis' 110
$colExe = New-GridColumn 'Executable' 'Application' 300
$colExe.AutoSizeMode = [System.Windows.Forms.DataGridViewAutoSizeColumnMode]::Fill
$colExe.MinimumWidth = 200
$colStartup = New-GridColumn 'Startup' 'Startup' 110
$colRestart = New-GridColumn 'Restart' 'Restart' 90
$grid.Columns.AddRange([System.Windows.Forms.DataGridViewColumn[]]@($colStatus, $colName, $colDisplay, $colType, $colExe, $colStartup, $colRestart))

$grid.Add_SelectionChanged({
    if (-not $script:SuppressEvents) { Update-LogView }
})
$grid.Add_CellDoubleClick({
    param($sender, $eventArgs)
    if ($eventArgs.RowIndex -ge 0) { Edit-ServiceUi }
})

$ctxMenu = New-Object System.Windows.Forms.ContextMenuStrip
$script:CtxEdit = New-Object System.Windows.Forms.ToolStripMenuItem
$script:CtxEdit.Text = 'Edit Service'
$script:CtxEdit.Add_Click({ Edit-ServiceUi })
[void]$ctxMenu.Items.Add($script:CtxEdit)

$script:CtxStart = New-Object System.Windows.Forms.ToolStripMenuItem
$script:CtxStart.Text = 'Start Service'
$script:CtxStart.Add_Click({ Start-ServiceUi })
[void]$ctxMenu.Items.Add($script:CtxStart)

$script:CtxStop = New-Object System.Windows.Forms.ToolStripMenuItem
$script:CtxStop.Text = 'Stop Service'
$script:CtxStop.Add_Click({ Stop-ServiceUi })
[void]$ctxMenu.Items.Add($script:CtxStop)

$script:CtxRestart = New-Object System.Windows.Forms.ToolStripMenuItem
$script:CtxRestart.Text = 'Restart Service'
$script:CtxRestart.Add_Click({ Restart-ServiceUi })
[void]$ctxMenu.Items.Add($script:CtxRestart)

$script:CtxInstall = New-Object System.Windows.Forms.ToolStripMenuItem
$script:CtxInstall.Text = 'Install Service'
$script:CtxInstall.Add_Click({ Install-ServiceUi })
[void]$ctxMenu.Items.Add($script:CtxInstall)

[void]$ctxMenu.Items.Add((New-Object System.Windows.Forms.ToolStripSeparator))

$script:CtxOpenLog = New-Object System.Windows.Forms.ToolStripMenuItem
$script:CtxOpenLog.Text = 'Buka File Log'
$script:CtxOpenLog.Add_Click({ Open-SelectedLogUi })
[void]$ctxMenu.Items.Add($script:CtxOpenLog)

[void]$ctxMenu.Items.Add((New-Object System.Windows.Forms.ToolStripSeparator))

$script:CtxDelete = New-Object System.Windows.Forms.ToolStripMenuItem
$script:CtxDelete.Text = 'Hapus Service'
$script:CtxDelete.Add_Click({ Delete-ServiceUi })
[void]$ctxMenu.Items.Add($script:CtxDelete)

$ctxMenu.Add_Opening({
    $svc = Get-SelectedService
    $hasService = $null -ne $svc
    $installed = $hasService -and (Test-ServiceExists $svc.name)

    $script:CtxEdit.Enabled = $hasService
    $script:CtxDelete.Enabled = $hasService
    $script:CtxOpenLog.Enabled = $hasService
    $script:CtxInstall.Enabled = $hasService -and -not $installed
    $script:CtxStart.Enabled = $installed
    $script:CtxStop.Enabled = $installed
    $script:CtxRestart.Enabled = $installed
})

$grid.ContextMenuStrip = $ctxMenu
$split.Panel1.Controls.Add($grid)

$logLayout = New-Object System.Windows.Forms.TableLayoutPanel
$logLayout.Dock = [System.Windows.Forms.DockStyle]::Fill
$logLayout.ColumnCount = 1
$logLayout.RowCount = 2
[void]$logLayout.ColumnStyles.Add((New-ColumnStyle -SizeType ([System.Windows.Forms.SizeType]::Percent) -Width 100))
[void]$logLayout.RowStyles.Add((New-RowStyle -SizeType ([System.Windows.Forms.SizeType]::Absolute) -Height 34))
[void]$logLayout.RowStyles.Add((New-RowStyle -SizeType ([System.Windows.Forms.SizeType]::Percent) -Height 100))

$logBar = New-Object System.Windows.Forms.FlowLayoutPanel
$logBar.Dock = [System.Windows.Forms.DockStyle]::Fill
$logBar.FlowDirection = [System.Windows.Forms.FlowDirection]::LeftToRight
$logBar.WrapContents = $false
$logBar.Padding = New-Object System.Windows.Forms.Padding -ArgumentList @(6, 5, 6, 2)

$logLabel = New-Object System.Windows.Forms.Label
$logLabel.Text = 'Log service: (belum ada yang dipilih)'
$logLabel.AutoSize = $true
$logLabel.Margin = New-Object System.Windows.Forms.Padding -ArgumentList @(0, 6, 14, 0)
[void]$logBar.Controls.Add($logLabel)
$script:LogLabel = $logLabel

$chkAuto = New-Object System.Windows.Forms.CheckBox
$chkAuto.Text = 'Auto refresh'
$chkAuto.AutoSize = $true
$chkAuto.Checked = $true
$chkAuto.Margin = New-Object System.Windows.Forms.Padding -ArgumentList @(0, 5, 12, 0)
[void]$logBar.Controls.Add($chkAuto)
$script:AutoRefreshLog = $chkAuto

$btnReload = New-Object System.Windows.Forms.Button
$btnReload.Text = 'Muat ulang'
$btnReload.AutoSize = $true
$btnReload.Margin = New-Object System.Windows.Forms.Padding -ArgumentList @(0, 2, 8, 0)
$btnReload.Add_Click({ Update-LogView })
[void]$logBar.Controls.Add($btnReload)

$btnOpenLog = New-Object System.Windows.Forms.Button
$btnOpenLog.Text = 'Buka file'
$btnOpenLog.AutoSize = $true
$btnOpenLog.Margin = New-Object System.Windows.Forms.Padding -ArgumentList @(0, 2, 0, 0)
$btnOpenLog.Add_Click({ Open-SelectedLogUi })
[void]$logBar.Controls.Add($btnOpenLog)

$logBox = New-Object System.Windows.Forms.TextBox
$logBox.Dock = [System.Windows.Forms.DockStyle]::Fill
$logBox.Multiline = $true
$logBox.ReadOnly = $true
$logBox.ScrollBars = [System.Windows.Forms.ScrollBars]::Both
$logBox.WordWrap = $false
$logBox.Font = New-Object System.Drawing.Font -ArgumentList 'Consolas', 9
$logBox.BackColor = [System.Drawing.Color]::FromArgb(30, 30, 30)
$logBox.ForeColor = [System.Drawing.Color]::FromArgb(220, 220, 220)
$logBox.BorderStyle = [System.Windows.Forms.BorderStyle]::None
$logBox.Margin = New-Object System.Windows.Forms.Padding -ArgumentList @(0)
$script:LogBox = $logBox

[void]$logLayout.Controls.Add($logBar, 0, 0)
[void]$logLayout.Controls.Add($logBox, 0, 1)
$split.Panel2.Controls.Add($logLayout)

$root = New-Object System.Windows.Forms.TableLayoutPanel
$root.Dock = [System.Windows.Forms.DockStyle]::Fill
$root.ColumnCount = 1
$root.RowCount = 4
[void]$root.ColumnStyles.Add((New-ColumnStyle -SizeType ([System.Windows.Forms.SizeType]::Percent) -Width 100))
[void]$root.RowStyles.Add((New-RowStyle -SizeType ([System.Windows.Forms.SizeType]::AutoSize) -Height 0))
[void]$root.RowStyles.Add((New-RowStyle -SizeType ([System.Windows.Forms.SizeType]::AutoSize) -Height 0))
[void]$root.RowStyles.Add((New-RowStyle -SizeType ([System.Windows.Forms.SizeType]::Percent) -Height 100))
[void]$root.RowStyles.Add((New-RowStyle -SizeType ([System.Windows.Forms.SizeType]::AutoSize) -Height 0))

$mainMenu.Margin = New-Object System.Windows.Forms.Padding -ArgumentList @(0)
$toolStrip.Margin = New-Object System.Windows.Forms.Padding -ArgumentList @(0)
$split.Margin = New-Object System.Windows.Forms.Padding -ArgumentList @(0)
$statusStrip.Margin = New-Object System.Windows.Forms.Padding -ArgumentList @(0)

[void]$root.Controls.Add($mainMenu, 0, 0)
[void]$root.Controls.Add($toolStrip, 0, 1)
[void]$root.Controls.Add($split, 0, 2)
[void]$root.Controls.Add($statusStrip, 0, 3)
$form.Controls.Add($root)

$script:StatusTimer = New-Object System.Windows.Forms.Timer
$script:StatusTimer.Interval = 3000
$script:StatusTimer.Add_Tick({ Update-StatusCells })
$script:StatusTimer.Start()

$script:LogTimer = New-Object System.Windows.Forms.Timer
$script:LogTimer.Interval = 2000
$script:LogTimer.Add_Tick({
    if ($null -ne $script:AutoRefreshLog -and $script:AutoRefreshLog.Checked) { Update-LogView }
})
$script:LogTimer.Start()

$form.Add_Shown({
    try {
        $script:Split.SplitterDistance = [int]($script:Split.Height * 0.62)
    } catch {
    }
    Refresh-Grid
    if (-not (Test-Path -LiteralPath $script:NssmPath)) {
        Show-Error "NSSM tidak ditemukan di:`r`n$($script:NssmPath)`r`n`r`nOperasi service NSSM tidak bisa dilakukan; service native tetap bisa dikelola."
    } elseif ($script:Services.Count -eq 0) {
        Set-AppStatus 'Daftar masih kosong. Klik Import untuk memindai service yang sudah terpasang.'
    } else {
        Set-AppStatus 'Siap.'
    }
})

$form.Add_FormClosing({
    try {
        if ($null -ne $script:StatusTimer) { $script:StatusTimer.Stop() }
        if ($null -ne $script:LogTimer) { $script:LogTimer.Stop() }
    } catch {
    }
})

try {
    [System.Windows.Forms.Application]::Run($form)
} catch {
    Write-AppLog "FATAL: $($_.Exception.Message)"
    Show-Error "Aplikasi berhenti karena error:`r`n$($_.Exception.Message)"
} finally {
    if ($null -ne $script:StatusTimer) { $script:StatusTimer.Stop() }
    if ($null -ne $script:LogTimer) { $script:LogTimer.Stop() }
    if ($null -ne $form -and -not $form.IsDisposed) { $form.Dispose() }
}
