[CmdletBinding()]
param(
    [string]$ConfigPath = '',
    [ValidateSet('Menu', 'List', 'Check', 'Auto')]
    [string]$Mode = 'Menu',
    [string]$Project = '',
    [switch]$All,
    [switch]$Menu
)

$ErrorActionPreference = 'Stop'
if ($Menu) { $Mode = 'Menu' }
if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
    $ConfigPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) 'projects.json'
}
$env:GCM_CREDENTIAL_STORE = 'dpapi'
$env:GCM_INTERACTIVE = 'never'
$env:GIT_TERMINAL_PROMPT = '0'

function Get-JsonValue {
    param($Object, [string]$Name, $Default = $null)
    if ($null -eq $Object) { return $Default }
    $prop = $Object.PSObject.Properties[$Name]
    if ($null -eq $prop -or $null -eq $prop.Value) { return $Default }
    return $prop.Value
}

function Test-LaravelProject {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) { return $false }
    if (-not (Test-Path -LiteralPath (Join-Path $Path '.git'))) { return $false }
    if (-not (Test-Path -LiteralPath (Join-Path $Path 'artisan') -PathType Leaf)) { return $false }
    if (-not (Test-Path -LiteralPath (Join-Path $Path 'composer.json') -PathType Leaf)) { return $false }
    return $true
}

if (-not (Test-Path -LiteralPath $ConfigPath -PathType Leaf)) {
    Write-Host "Config tidak ditemukan: $ConfigPath"
    exit 1
}

try {
    $config = Get-Content -LiteralPath $ConfigPath -Raw -Encoding UTF8 | ConvertFrom-Json
} catch {
    Write-Host "Config tidak valid: $($_.Exception.Message)"
    exit 1
}

$scanRoots = @(Get-JsonValue $config 'scanRoots' @('D:\Projects\PHPProjects'))
if ($scanRoots.Count -eq 0) { $scanRoots = @('D:\Projects\PHPProjects') }
$allowedClientIPs = @(Get-JsonValue $config 'allowedClientIPs' @('127.0.0.1', '::1'))
$gitPath = [string](Get-JsonValue $config 'gitPath' 'C:\laragon\bin\git\cmd\git.exe')
$phpPath = [string](Get-JsonValue $config 'phpPath' 'C:\laragon\bin\php\php-8.3.16-Win32-vs16-x64\php.exe')
$composerPath = [string](Get-JsonValue $config 'composerPath' 'C:\laragon\bin\composer\composer.bat')
$npmPath = [string](Get-JsonValue $config 'npmPath' 'C:\Program Files\nodejs\npm.cmd')
$logDir = [string](Get-JsonValue $config 'logDir' (Join-Path $PSScriptRoot 'logs'))
$projectsConfig = Get-JsonValue $config 'projects' $null

$runStamp = Get-Date -Format 'yyyyMMdd-HHmmss'
try {
    if (-not (Test-Path -LiteralPath $logDir -PathType Container)) {
        New-Item -ItemType Directory -Path $logDir -Force | Out-Null
    }
} catch {
}
$script:RunLog = Join-Path $logDir ('project-update-{0}.log' -f $runStamp)
$script:ActiveLog = ''

function Write-Log {
    param([string]$Message)
    $line = '{0} {1}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $Message
    Write-Host $line
    try { Add-Content -LiteralPath $script:RunLog -Value $line -Encoding UTF8 } catch { }
    if ($script:ActiveLog) {
        try { Add-Content -LiteralPath $script:ActiveLog -Value $line -Encoding UTF8 } catch { }
    }
}

function Invoke-ProcessCapture {
    param(
        [string]$FilePath,
        [string[]]$Arguments,
        [string]$WorkingDirectory,
        [int]$TimeoutSeconds
    )

    $argLine = ($Arguments | ForEach-Object {
            if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ }
        }) -join ' '

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $FilePath
    $psi.Arguments = $argLine
    $psi.WorkingDirectory = $WorkingDirectory
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.CreateNoWindow = $true

    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi
    try {
        if (-not $proc.Start()) {
            throw "Gagal menjalankan: $FilePath"
        }
        $outTask = $proc.StandardOutput.ReadToEndAsync()
        $errTask = $proc.StandardError.ReadToEndAsync()
        if (-not $proc.WaitForExit($TimeoutSeconds * 1000)) {
            try {
                & "$env:SystemRoot\System32\taskkill.exe" /PID $proc.Id /T /F 2>&1 | Out-Null
            } catch {
                try { $proc.Kill() } catch { }
            }
            return [pscustomobject]@{ TimedOut = $true; ExitCode = 124; StdOut = ''; StdErr = '' }
        }
        $proc.WaitForExit()
        $stdout = [string]$outTask.Result
        $stderr = [string]$errTask.Result
        return [pscustomobject]@{
            TimedOut = $false
            ExitCode = [int]$proc.ExitCode
            StdOut = $stdout.Trim()
            StdErr = $stderr.Trim()
        }
    } finally {
        try { $proc.Dispose() } catch { }
    }
}

$clientIp = ''
if ($env:SSH_CONNECTION) {
    $clientIp = ([string]$env:SSH_CONNECTION -split '\s+')[0]
} elseif ($env:SSH_CLIENT) {
    $clientIp = ([string]$env:SSH_CLIENT -split '\s+')[0]
}
if ($clientIp -like '::ffff:*') { $clientIp = $clientIp.Substring(7) }
if ($clientIp -and $allowedClientIPs.Count -gt 0 -and -not ($allowedClientIPs -contains $clientIp)) {
    Write-Host "Akses ditolak untuk IP $clientIp. Tambahkan IP ini ke allowedClientIPs di $ConfigPath."
    exit 3
}

function Get-GitBranch {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $gitPath -PathType Leaf)) { return '' }
    $value = & $gitPath -C $Path rev-parse --abbrev-ref HEAD 2>$null
    if ($LASTEXITCODE -ne 0) { return '' }
    return ([string]$value).Trim()
}

function Get-ProjectOverride {
    param([string]$Name)
    if ($null -eq $projectsConfig) { return $null }
    $prop = $projectsConfig.PSObject.Properties[$Name]
    if ($null -eq $prop) { return $null }
    return $prop.Value
}

function New-ProjectInfo {
    param([string]$Id, [string]$Path)

    $override = Get-ProjectOverride $Id
    $currentBranch = Get-GitBranch $Path
    $configuredBranch = [string](Get-JsonValue $override 'branch' '')
    if ([string]::IsNullOrWhiteSpace($configuredBranch)) {
        $branch = $currentBranch
    } else {
        $branch = $configuredBranch
    }

    $enabled = $true
    $composerMode = 'auto'
    $buildMode = 'auto'
    $queueRestart = $false
    $postUpdateCommand = ''

    if ($null -ne $override) {
        if ($null -ne $override.PSObject.Properties['enabled']) { $enabled = [bool]$override.enabled }
        $composerMode = ([string](Get-JsonValue $override 'composer' 'auto')).ToLowerInvariant()
        $buildMode = ([string](Get-JsonValue $override 'build' 'auto')).ToLowerInvariant()
        if ($composerMode -eq 'never') { $composerMode = 'off' }
        if ($buildMode -eq 'never') { $buildMode = 'off' }
        if ($composerMode -notin @('auto', 'always', 'off')) { $composerMode = 'auto' }
        if ($buildMode -notin @('auto', 'always', 'off')) { $buildMode = 'auto' }
        if ($null -ne $override.PSObject.Properties['queueRestart']) { $queueRestart = [bool]$override.queueRestart }
        $postUpdateCommand = [string](Get-JsonValue $override 'postUpdateCommand' '')
    }

    return [pscustomobject]@{
        Id = $Id
        Name = $Id
        Path = $Path
        Branch = $branch
        ConfiguredBranch = $configuredBranch
        CurrentBranch = $currentBranch
        Enabled = $enabled
        Composer = $composerMode
        Build = $buildMode
        QueueRestart = $queueRestart
        PostUpdateCommand = $postUpdateCommand
    }
}

function Get-ProjectList {
    $list = New-Object System.Collections.ArrayList
    $seen = @{}

    foreach ($root in $scanRoots) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
        foreach ($dir in @(Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue)) {
            if (-not (Test-LaravelProject $dir.FullName)) { continue }
            $id = [string]$dir.Name
            if ($seen.ContainsKey($id)) { continue }
            $seen[$id] = $true
            [void]$list.Add((New-ProjectInfo -Id $id -Path $dir.FullName))
        }
    }

    if ($null -ne $projectsConfig) {
        foreach ($prop in $projectsConfig.PSObject.Properties) {
            if ($seen.ContainsKey([string]$prop.Name)) { continue }
            $path = [string](Get-JsonValue $prop.Value 'path' '')
            if (-not [string]::IsNullOrWhiteSpace($path) -and (Test-LaravelProject $path)) {
                $seen[[string]$prop.Name] = $true
                [void]$list.Add((New-ProjectInfo -Id ([string]$prop.Name) -Path $path))
            }
        }
    }

    return @($list | Sort-Object Name)
}

function Invoke-ProjectUpdate {
    param($ProjectInfo, [string]$Action)

    $p = $ProjectInfo
    $script:ActiveLog = ''
    try {
        $projectLogDir = Join-Path $p.Path 'storage\logs'
        if (-not (Test-Path -LiteralPath $projectLogDir -PathType Container)) {
            New-Item -ItemType Directory -Path $projectLogDir -Force | Out-Null
        }
        $script:ActiveLog = Join-Path $projectLogDir ('project-update-{0}.log' -f $runStamp)
    } catch {
        $script:ActiveLog = ''
    }

    $pushed = $false
    try {
        Write-Log ('===== {0}: {1} =====' -f $p.Name, $Action.ToUpperInvariant())
        if (-not (Test-LaravelProject $p.Path)) {
            throw "Project bukan Laravel yang valid: $($p.Path)"
        }
        if (-not (Test-Path -LiteralPath $gitPath -PathType Leaf)) {
            throw "Git tidak ditemukan: $gitPath"
        }

        Push-Location $p.Path
        $pushed = $true

        $statusLines = @(& $gitPath status --porcelain 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw 'Gagal membaca status Git.'
        }
        if ($statusLines.Count -gt 0) {
            Write-Log 'Working tree tidak bersih; project dilewati.'
            $statusLines | Select-Object -First 10 | ForEach-Object { Write-Log ([string]$_) }
            return 'dirty'
        }

        $currentBranch = (& $gitPath rev-parse --abbrev-ref HEAD 2>&1).Trim()
        if ($LASTEXITCODE -ne 0) {
            throw 'Gagal membaca branch Git.'
        }
        if (-not [string]::IsNullOrWhiteSpace($p.ConfiguredBranch) -and $currentBranch -ne $p.ConfiguredBranch) {
            throw "Branch aktif '$currentBranch', bukan '$($p.ConfiguredBranch)'."
        }
        $branch = $currentBranch
        Write-Log "Branch: $branch"

        $before = (& $gitPath rev-parse HEAD 2>&1).Trim()
        if ($LASTEXITCODE -ne 0) {
            throw 'Gagal membaca SHA HEAD.'
        }
        Write-Log "SHA sebelum: $before"

        $probe = Invoke-ProcessCapture -FilePath $gitPath -Arguments @('-C', $p.Path, 'ls-remote', '--heads', 'origin') -WorkingDirectory $p.Path -TimeoutSeconds 30
        if ($probe.TimedOut) {
            throw 'Preflight remote timeout. Cek kredensial/network Git untuk akun service.'
        }
        if ($probe.ExitCode -ne 0 -or $probe.StdErr -match '(?i)fatal:|could not read Username|Authentication failed|terminal prompts disabled') {
            $probeMessage = $probe.StdErr
            if ([string]::IsNullOrWhiteSpace($probeMessage)) { $probeMessage = $probe.StdOut }
            throw ('Akses remote gagal. Kredensial Git belum ada/tidak valid. Login sekali di server 160 sebagai EGOV22 lewat RDP/console, atau pakai PAT/SSH key; browser tidak bisa muncul via SSH. ' + $probeMessage).Trim()
        }

        Write-Log 'git fetch --prune origin'
        $fetch = Invoke-ProcessCapture -FilePath $gitPath -Arguments @('-C', $p.Path, 'fetch', '--prune', 'origin') -WorkingDirectory $p.Path -TimeoutSeconds 120
        if ($fetch.StdOut) { Write-Log $fetch.StdOut }
        if ($fetch.StdErr) { Write-Log $fetch.StdErr }
        if ($fetch.TimedOut) {
            throw 'git fetch timeout setelah 120 detik. Cek kredensial/network Git.'
        }
        if ($fetch.ExitCode -ne 0 -or $fetch.StdErr -match '(?i)fatal:|could not read Username|Authentication failed|terminal prompts disabled') {
            throw 'git fetch gagal. Kredensial Git belum ada/tidak valid. Login sekali di server 160 sebagai EGOV22 lewat RDP/console, atau pakai PAT/SSH key; browser tidak bisa muncul via SSH.'
        }

        & $gitPath rev-parse --verify "origin/$branch" > $null 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "Remote origin/$branch tidak ditemukan."
        }

        $incoming = @(& $gitPath log --oneline "HEAD..origin/$branch" 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw 'Gagal membaca commit masuk.'
        }
        $previewChanged = @(& $gitPath diff --name-only "HEAD..origin/$branch" 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw 'Gagal membaca daftar file berubah.'
        }

        if ($incoming.Count -eq 0) {
            Write-Log 'Sudah terbaru; tidak ada commit baru.'
            return 'up-to-date'
        }

        Write-Log "Commit baru ($($incoming.Count)):"
        $incoming | Select-Object -First 30 | ForEach-Object { Write-Log ([string]$_) }
        Write-Log 'File berubah:'
        $previewChanged | Select-Object -First 40 | ForEach-Object { Write-Log ([string]$_) }

        $migrationPreview = @($previewChanged | Where-Object { $_ -like 'database/migrations/*' })
        if ($migrationPreview.Count -gt 0) {
            Write-Log 'INFO MIGRASI: perubahan migration terdeteksi; update otomatis akan dihentikan.'
            $migrationPreview | ForEach-Object { Write-Log ("MIGRASI: {0}" -f $_) }
        }

        if ($Action -eq 'Check') {
            Write-Log 'Cek selesai. Tidak ada file yang diubah.'
            return 'check'
        }

        if ($migrationPreview.Count -gt 0) {
            Write-Log 'DIBATALKAN: migration harus dijalankan manual. Tidak ada pull yang dilakukan.'
            return 'blocked-migration'
        }

        Write-Log ('git merge --ff-only origin/{0}' -f $branch)
        & $gitPath merge --ff-only "origin/$branch"
        if ($LASTEXITCODE -ne 0) {
            throw 'git merge --ff-only gagal.'
        }

        $after = (& $gitPath rev-parse HEAD 2>&1).Trim()
        if ($LASTEXITCODE -ne 0) {
            throw 'Gagal membaca SHA HEAD setelah pull.'
        }
        if ($before -eq $after) {
            Write-Log 'Sudah terbaru; tidak ada perubahan.'
            return 'up-to-date'
        }

        $changed = @(& $gitPath diff --name-only $before $after 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw 'Gagal membaca file yang berubah.'
        }
        Write-Log "Commit baru: $before..$after"

        $composerChanged = @($changed | Where-Object { $_ -in @('composer.json', 'composer.lock') }).Count -gt 0
        $runComposer = ($p.Composer -eq 'always') -or ($p.Composer -eq 'auto' -and $composerChanged)
        if ($runComposer) {
            if (-not (Test-Path -LiteralPath $composerPath -PathType Leaf)) {
                throw "Composer tidak ditemukan: $composerPath"
            }
            Write-Log 'composer install'
            & $composerPath install --no-dev --optimize-autoloader --no-interaction
            if ($LASTEXITCODE -ne 0) {
                throw 'composer install gagal.'
            }
        }

        $packageLockChanged = $changed -contains 'package-lock.json'
        $packageChanged = $changed -contains 'package.json'
        $frontendChanged = @($changed | Where-Object {
                $_ -like 'resources/*' -or
                $_ -in @('vite.config.ts', 'vite.config.js', 'tsconfig.json', 'package.json', 'package-lock.json')
            }).Count -gt 0
        $runBuild = ($p.Build -eq 'always') -or ($p.Build -eq 'auto' -and $frontendChanged)
        if ($runBuild) {
            if (-not (Test-Path -LiteralPath $npmPath -PathType Leaf)) {
                throw "npm tidak ditemukan: $npmPath"
            }
            if ($packageLockChanged -or ($p.Build -eq 'always' -and (Test-Path -LiteralPath (Join-Path $p.Path 'package-lock.json')))) {
                Write-Log 'npm ci'
                & $npmPath ci
                if ($LASTEXITCODE -ne 0) {
                    throw 'npm ci gagal.'
                }
            } elseif ($packageChanged -or $p.Build -eq 'always') {
                Write-Log 'npm install'
                & $npmPath install
                if ($LASTEXITCODE -ne 0) {
                    throw 'npm install gagal.'
                }
            }
            Write-Log 'npm run build'
            & $npmPath run build
            if ($LASTEXITCODE -ne 0) {
                throw 'npm run build gagal.'
            }
        }

        $cacheFiles = @('bootstrap/cache/config.php', 'bootstrap/cache/routes.php', 'bootstrap/cache/routes-v7.php') |
            Where-Object { Test-Path -LiteralPath (Join-Path $p.Path $_) }
        if ($cacheFiles) {
            if (-not (Test-Path -LiteralPath $phpPath -PathType Leaf)) {
                throw "PHP tidak ditemukan: $phpPath"
            }
            Write-Log 'php artisan optimize:clear'
            & $phpPath artisan optimize:clear
            if ($LASTEXITCODE -ne 0) {
                throw 'php artisan optimize:clear gagal.'
            }
        }

        $codeChanged = @($changed | Where-Object {
                $_ -like 'app/*' -or
                $_ -like 'bootstrap/*' -or
                $_ -like 'config/*' -or
                $_ -like 'database/*' -or
                $_ -like 'lang/*' -or
                $_ -like 'resources/*' -or
                $_ -like 'routes/*' -or
                $_ -in @('composer.json', 'composer.lock', 'package.json', 'package-lock.json')
            }).Count -gt 0
        if ($p.QueueRestart -and $codeChanged) {
            Write-Log 'php artisan queue:restart'
            & $phpPath artisan queue:restart
            if ($LASTEXITCODE -ne 0) {
                throw 'php artisan queue:restart gagal.'
            }
        }

        if (-not [string]::IsNullOrWhiteSpace($p.PostUpdateCommand)) {
            Write-Log 'Post-update command'
            & powershell.exe -NoProfile -ExecutionPolicy Bypass -Command $p.PostUpdateCommand
            if ($LASTEXITCODE -ne 0) {
                throw 'Post-update command gagal.'
            }
        }

        Write-Log 'DEPLOY SELESAI'
        Write-Log ("Rollback kode: git reset --hard {0}" -f $before)
        return 'updated'
    } catch {
        Write-Log ("GAGAL: {0}" -f $_.Exception.Message)
        Write-Log ("STACK: {0}" -f $_.ScriptStackTrace)
        return 'failed'
    } finally {
        if ($pushed) { Pop-Location }
        $script:ActiveLog = ''
    }
}

function Show-ProjectMenu {
    while ($true) {
        $projects = @(Get-ProjectList)
        if ($projects.Count -eq 0) {
            Write-Host 'Tidak ada project Laravel yang terdeteksi.'
            return
        }

        Write-Host ''
        Write-Host '============================================================'
        Write-Host ' PROJECT UPDATE'
        Write-Host (' User   : {0} @ {1}' -f [System.Security.Principal.WindowsIdentity]::GetCurrent().Name, $env:COMPUTERNAME)
        Write-Host '============================================================'
        for ($i = 0; $i -lt $projects.Count; $i++) {
            $p = $projects[$i]
            $state = 'aktif'
            if (-not $p.Enabled) { $state = 'nonaktif' }
            Write-Host ('  {0}. {1} [{2}] ({3})' -f ($i + 1), $p.Name, $p.Branch, $state)
        }
        Write-Host '  0. Keluar'

        $choice = ([string](Read-Host 'Pilih project')).Trim()
        if ([string]::IsNullOrWhiteSpace($choice) -or $choice -eq '0') { return }
        $index = 0
        if (-not [int]::TryParse($choice, [ref]$index)) {
            Write-Host 'Pilihan tidak valid.'
            continue
        }
        if ($index -lt 1 -or $index -gt $projects.Count) {
            Write-Host 'Pilihan tidak valid.'
            continue
        }

        $p = $projects[$index - 1]
        Write-Host ''
        Write-Host ('Project: {0}' -f $p.Name)
        Write-Host ('Path   : {0}' -f $p.Path)
        Write-Host ('Branch : {0}' -f $p.Branch)
        Write-Host '  1. Cek update (tanpa mengubah file)'
        Write-Host '  2. Update (pull + composer + build)'
        Write-Host '  0. Batal'

        $action = ([string](Read-Host 'Pilih aksi')).Trim()
        if ([string]::IsNullOrWhiteSpace($action) -or $action -eq '0') { continue }

        if ($action -eq '1') {
            [void](Invoke-ProjectUpdate -ProjectInfo $p -Action 'Check')
        } elseif ($action -eq '2') {
            $confirm = ([string](Read-Host ('Update {0} sekarang? Ketik YA' -f $p.Name))).Trim()
            if ($confirm -ne 'YA') {
                Write-Host 'Dibatalkan.'
                continue
            }
            [void](Invoke-ProjectUpdate -ProjectInfo $p -Action 'Auto')
        } else {
            Write-Host 'Aksi tidak valid.'
            continue
        }

        Write-Host ''
        [void](Read-Host 'Tekan Enter untuk kembali ke menu')
    }
}

if ($Mode -eq 'Menu') {
    Show-ProjectMenu
    exit 0
}

$projects = @(Get-ProjectList)
if ($projects.Count -eq 0) {
    Write-Host 'Tidak ada project Laravel yang terdeteksi.'
    exit 1
}

if ($Mode -eq 'List') {
    Write-Host 'Project Laravel:'
    foreach ($p in $projects) {
        $state = 'aktif'
        if (-not $p.Enabled) { $state = 'nonaktif' }
        Write-Host ('  {0} [{1}] ({2}) -> {3}' -f $p.Name, $p.Branch, $state, $p.Path)
    }
    exit 0
}

$targets = @()
if (-not [string]::IsNullOrWhiteSpace($Project)) {
    $targets = @($projects | Where-Object { [string]::Equals([string]$_.Name, $Project, [System.StringComparison]::OrdinalIgnoreCase) })
    if ($targets.Count -eq 0) {
        Write-Host "Project '$Project' tidak ditemukan."
        exit 1
    }
} else {
    $targets = @($projects | Where-Object { $_.Enabled })
    if ($targets.Count -eq 0) {
        Write-Host 'Tidak ada project aktif.'
        exit 1
    }
}

$failed = 0
foreach ($p in $targets) {
    $result = Invoke-ProjectUpdate -ProjectInfo $p -Action $Mode
    if ($result -eq 'failed') { $failed++ }
}

if ($failed -gt 0) { exit 1 }
exit 0



