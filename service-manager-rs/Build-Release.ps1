[CmdletBinding()]
param(
    [switch]$Bundle
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $root
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

    Write-Host "Build release $name..."
    & cargo build --release -p sm-gui -p sm-updater
    if ($LASTEXITCODE -ne 0) { throw 'cargo build gagal.' }

    $stage = Join-Path $dist $name
    if (Test-Path -LiteralPath $stage) {
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
    New-Item -ItemType Directory -Path $stage -Force | Out-Null

    Copy-Item -LiteralPath (Join-Path $root 'target\release\sm-gui.exe') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $root 'target\release\sm-updater.exe') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $root 'target\release\sm-diag.exe') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $repoRoot 'services.example.json') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $repoRoot 'projects.example.json') -Destination $stage

    Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination (Join-Path $stage 'README.md')
    $docs = Join-Path $stage 'docs'
    New-Item -ItemType Directory -Path $docs -Force | Out-Null
    foreach ($doc in @('RUST-GUI.md', 'RUST-UPDATER.md', 'RUST-SECURITY.md', 'RUST-RELEASE.md')) {
        $path = Join-Path $repoRoot (Join-Path 'docs' $doc)
        if (Test-Path -LiteralPath $path) {
            Copy-Item -LiteralPath $path -Destination $docs
        }
    }

    $runNotes = @"
Service Manager RS $version - Portable
======================================

1. Salin services.example.json menjadi services.json dan sesuaikan path.
2. Salin projects.example.json menjadi projects.json bila memakai updater.
3. Letakkan nssm.exe di C:\tools\nssm\nssm.exe (atau ..\nssm\nssm.exe).
4. Jalankan sm-gui.exe sebagai Administrator.
5. Updater: sm-updater.exe list / check / update.
6. Simpan kredensial Git: sm-updater.exe credential store --help
7. Diagnosa: sm-diag.exe

Config dibaca dari folder exe. Set SERVICE_MANAGER_HOME untuk folder lain.
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
