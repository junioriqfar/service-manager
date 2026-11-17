[CmdletBinding()]
param(
    [switch]$Bundle
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = $root
$dist = Join-Path $root 'dist'

Push-Location $root
try {
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($null -eq $cargo) {
        throw 'cargo tidak ditemukan. Install Rust dari https://rustup.rs terlebih dahulu.'
    }

    $manifest = Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') -Raw
    $match = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) { throw 'Versi workspace tidak ditemukan di Cargo.toml.' }
    $version = $match.Groups[1].Value
    $name = "ServiceManager-RS-$version"

    $ui = Join-Path $root 'crates\sm-gui\ui'
    if (-not (Test-Path -LiteralPath $ui)) {
        throw "Frontend folder tidak ditemukan: $ui"
    }
    $npm = Get-Command npm.cmd -ErrorAction SilentlyContinue
    if ($null -eq $npm) {
        $npm = Get-Command npm -ErrorAction SilentlyContinue
    }
    if ($null -eq $npm) {
        throw 'npm tidak ditemukan. Install Node.js dari https://nodejs.org/ terlebih dahulu.'
    }

    Write-Host 'Build frontend React + Vite...'
    Push-Location $ui
    try {
        if (Test-Path -LiteralPath (Join-Path $ui 'package-lock.json')) {
            & $npm.Source ci
            if ($LASTEXITCODE -ne 0) { throw 'npm ci gagal.' }
        } else {
            & $npm.Source install
            if ($LASTEXITCODE -ne 0) { throw 'npm install gagal.' }
        }
        & $npm.Source run build
        if ($LASTEXITCODE -ne 0) { throw 'npm run build gagal.' }
    } finally {
        Pop-Location
    }

    Write-Host "Build release $name..."
    & cargo build --release -p sm-gui -p sm-updater -p sm-trigger
    if ($LASTEXITCODE -ne 0) { throw 'cargo build gagal.' }

    $stage = Join-Path $dist $name
    if (Test-Path -LiteralPath $stage) {
        try {
            Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction Stop
        } catch {
            throw "Cannot replace '$stage'. Close sm-gui.exe (or any app using the dist folder) and retry."
        }
    }
    New-Item -ItemType Directory -Path $stage -Force | Out-Null

    Copy-Item -LiteralPath (Join-Path $root 'target\release\sm-gui.exe') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $root 'target\release\sm-updater.exe') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $root 'target\release\sm-trigger.exe') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $root 'target\release\sm-diag.exe') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $repoRoot 'services.example.json') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $repoRoot 'projects.example.json') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $repoRoot 'settings.example.json') -Destination $stage
    $dataStage = Join-Path $stage 'data'
    New-Item -ItemType Directory -Path $dataStage -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repoRoot 'services.example.json') -Destination (Join-Path $dataStage 'services.example.json')
    Copy-Item -LiteralPath (Join-Path $repoRoot 'projects.example.json') -Destination (Join-Path $dataStage 'projects.example.json')
    Copy-Item -LiteralPath (Join-Path $repoRoot 'settings.example.json') -Destination (Join-Path $dataStage 'settings.example.json')
    New-Item -ItemType Directory -Path (Join-Path $stage 'backup') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $stage 'logs') -Force | Out-Null

    $scriptsStage = Join-Path $stage 'scripts'
    New-Item -ItemType Directory -Path $scriptsStage -Force | Out-Null
    foreach ($script in @('Invoke-UpdaterTask.ps1', 'Run-UpdaterTaskAction.ps1', 'Install-UpdaterTask.ps1')) {
        Copy-Item -LiteralPath (Join-Path $root (Join-Path 'scripts' $script)) -Destination $scriptsStage
    }

    Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination (Join-Path $stage 'README.md')
    $docs = Join-Path $stage 'docs'
    New-Item -ItemType Directory -Path $docs -Force | Out-Null
    foreach ($doc in @('CONFIGURATION.md', 'RUST-README.md', 'RUST-GUI.md', 'RUST-SERVICES.md', 'RUST-UPDATER.md', 'RUST-UPDATER-PLAN.md', 'RUST-UPDATER-REGISTRY-PLAN.md', 'RUST-TRIGGER.md', 'RUST-SETTINGS-PLAN.md', 'RUST-OPERATIONS-PLAN.md', 'RUST-SECURITY.md', 'RUST-RELEASE.md')) {
        $path = Join-Path $repoRoot (Join-Path 'docs' $doc)
        if (Test-Path -LiteralPath $path) {
            Copy-Item -LiteralPath $path -Destination $docs
        }
    }

    $runNotes = @"
Service Manager RS $version - Portable
======================================

1. Salin data\services.example.json menjadi data\services.json dan sesuaikan path.
2. Salin data\projects.example.json menjadi data\projects.json bila memakai updater.
3. Salin data\settings.example.json menjadi data\settings.json lalu sesuaikan path tool.
4. Letakkan nssm.exe di C:\tools\nssm\nssm.exe (atau ..\nssm\nssm.exe).
5. Jalankan sm-gui.exe sebagai Administrator.
6. Updater registry: sm-updater.exe app list / run --all / run --all --check.
7. Trigger SSH dari client lain: sm-trigger.exe --host HOST --user USER --all.
8. Elevated trigger: daftarkan Scheduled Task dengan scripts\Install-UpdaterTask.ps1 (Administrator).
9. Simpan kredensial Git: sm-updater.exe credential store --help
10. Diagnosa: sm-diag.exe
11. Backup config tersimpan di folder backup; log aplikasi di folder logs.

Config dibaca dari <folder exe>\data. Set SERVICE_MANAGER_HOME untuk folder lain.
Dokumentasi ada di folder docs.
"@
    Set-Content -LiteralPath (Join-Path $stage 'RUN.txt') -Value $runNotes -Encoding UTF8

    $zip = Join-Path $dist "$name.zip"
    if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force

    if ($Bundle) {
        $tauri = Get-Command cargo-tauri -ErrorAction SilentlyContinue
        if ($null -eq $tauri) {
            Write-Warning 'cargo-tauri tidak ditemukan. Install dengan: cargo install tauri-cli --version "^2"'
        } else {
            Push-Location (Join-Path $root 'crates\sm-gui')
            try {
                & cargo tauri build
                if ($LASTEXITCODE -ne 0) { throw 'cargo tauri build gagal.' }
            } finally {
                Pop-Location
            }
        }
    }

    Write-Host "Folder portable : $stage"
    Write-Host "Arsip ZIP       : $zip"
} finally {
    Pop-Location
}
