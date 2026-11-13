# Build dan Rilis Rust

Panduan build, packaging, CI, dan deployment untuk `service-manager-rs`.

## Prasyarat

| Komponen | Keterangan |
| --- | --- |
| Rust | stable + `rustfmt` + `clippy` (rustup) |
| Visual Studio Build Tools | MSVC C++ workload (Windows) |
| WebView2 Runtime | Untuk GUI Tauri (umumnya sudah ada di Windows 10/11/Server modern) |
| NSSM | `C:\tools\nssm\nssm.exe` atau `..\nssm\nssm.exe` |
| Tauri CLI (opsional) | Untuk installer NSIS: `cargo install tauri-cli --version "^2"` |

## Portable ZIP

```powershell
cd service-manager-rs
.\Build-Release.ps1
```

Output:

```
service-manager-rs\dist\ServiceManager-RS-0.1.0\
├─ sm-gui.exe
├─ sm-updater.exe
├─ sm-diag.exe
├─ services.example.json
├─ projects.example.json
├─ RUN.txt
├─ README.md
└─ docs\*.md
service-manager-rs\dist\ServiceManager-RS-0.1.0.zip
```

Langkah manual yang sama:

```powershell
cargo build --release -p sm-gui -p sm-updater
```

Binary ada di `target\release\`.

## Installer NSIS

Dengan Tauri CLI:

```powershell
cargo install tauri-cli --version "^2"
cd service-manager-rs
.\Build-Release.ps1 -Bundle
```

Atau langsung:

```powershell
cd service-manager-rs\crates\sm-gui
cargo tauri build
```

Output installer berada di:

```
service-manager-rs\target\release\bundle\nsis\
```

Konfigurasi bundle ada di `crates/sm-gui/tauri.conf.json`:

- `productName`: Service Manager
- `identifier`: com.servicemanager.rs
- `targets`: `["nsis"]`
- icon: `icons/icon.ico`

Catatan:

- Build installer memerlukan download NSIS oleh Tauri CLI; pastikan akses internet/proxy tersedia.
- Installer tidak otomatis menjalankan sebagai Administrator; GUI menampilkan banner elevasi.
- WebView2 Runtime harus tersedia di target.

## Code Signing (Opsional)

Tanpa code signing, Windows SmartScreen dapat menampilkan peringatan.

1. Siapkan sertifikat code signing (file `.pfx` atau hardware token).
2. Tandatangani `sm-gui.exe`, `sm-updater.exe`, dan installer:
   ```powershell
   signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /f cert.pfx /p $env:CERT_PASSWORD path\to\file.exe
   ```
3. Simpan sertifikat/password di secret CI, **jangan** di repositori.
4. Verifikasi:
   ```powershell
   signtool verify /pa /v path\to\file.exe
   ```

Tauri mendukung konfigurasi signing melalui environment variable saat `tauri build`; lihat dokumentasi Tauri untuk detail.

## CI/CD

### `rust.yml` (push/PR)

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `Build-Release.ps1` (portable ZIP)
- Upload artifact `service-manager-rs-portable`

### `release.yml` (tag `v*`)

- Test
- Build portable ZIP
- Publish GitHub Release dengan file ZIP

Contoh rilis:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

## Deployment

1. Ekstrak ZIP ke folder aplikasi, misalnya `C:\tools\ServiceManager-RS`.
2. Salin contoh config:
   ```powershell
   Copy-Item services.example.json services.json
   Copy-Item projects.example.json projects.json
   ```
3. Sesuaikan path NSSM/PHP/Composer/npm/project.
4. Letakkan `nssm.exe` di `C:\tools\nssm\nssm.exe` atau `..\nssm\nssm.exe`.
5. Jalankan `sm-gui.exe` sebagai Administrator.
6. Simpan kredensial Git:
   ```powershell
   .\sm-updater.exe credential store --project D:\Projects\PHPProjects\open-po
   ```
7. Uji:
   ```powershell
   .\sm-updater.exe list
   .\sm-updater.exe check --project kasirin
   .\sm-diag.exe
   ```

Config dibaca dari folder executable. Gunakan `SERVICE_MANAGER_HOME` untuk memakai folder config lain (misalnya saat development).

## Migrasi dari PowerShell

- Format `services.json` dan `projects.json` sama (versi 1).
- Backup config lama sebelum cutover.
- Jalankan GUI Rust dan PowerShell berdampingan; keduanya membaca config yang sama.
- Jangan jalankan operasi tulis bersamaan pada config yang sama.
- Rollback: gunakan kembali `ServiceManager.ps1` + config/backup yang sama.

## Verifikasi Rilis

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (semua test hijau)
- [ ] `Build-Release.ps1` menghasilkan ZIP dan isinya lengkap
- [ ] `sm-gui.exe` terbuka, memuat service, dan batch Start/Stop/Restart menampilkan progress
- [ ] `sm-updater.exe list/check` berjalan
- [ ] `sm-diag.exe` menampilkan path, status admin, NSSM, dan service
- [ ] Credential helper menyimpan PAT dan `ls-remote` berhasil
- [ ] Log tidak memuat token/password
- [ ] Config contoh tidak berisi path/rahasia nyata

## Keterbatasan

- Installer NSIS belum ditandatangani otomatis.
- Belum ada auto-update aplikasi.
- CI tidak menjalankan operasi tulis Windows Service (butuh Administrator); test tersebut dijalankan manual di server uji.
