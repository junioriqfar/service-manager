# Build dan Rilis Rust

Panduan build, packaging, CI, dan deployment untuk workspace Rust di root repositori.

## Prasyarat

| Komponen | Keterangan |
| --- | --- |
| Rust | stable + `rustfmt` + `clippy` (rustup) |
| Visual Studio Build Tools | MSVC C++ workload (Windows) |
| WebView2 Runtime | Untuk GUI Tauri (umumnya sudah ada di Windows 10/11/Server modern) |
| Node.js | Node.js 22+ dan npm untuk build frontend React/shadCN |
| NSSM | `C:\tools\nssm\nssm.exe` atau `..\nssm\nssm.exe` |
| Tauri CLI (opsional) | Untuk installer NSIS: `cargo install tauri-cli --version "^2"` |

## Portable ZIP

```powershell
.\Build-Release.ps1
```

Output:

```
dist\ServiceManager-RS-0.1.0\
├─ sm-gui.exe
├─ sm-updater.exe
├─ sm-diag.exe
├─ data\
│  ├─ services.example.json
│  └─ projects.example.json
├─ services.example.json
├─ projects.example.json
├─ backup\
├─ logs\
├─ RUN.txt
├─ README.md
└─ docs\*.md
dist\ServiceManager-RS-0.1.0.zip
```

Langkah manual yang sama:

```powershell
cd crates\sm-gui\ui
npm ci
npm run build
cd ..\..\..
cargo build --release -p sm-gui -p sm-updater
```

Binary ada di `target\release\`.

## Installer NSIS

Dengan Tauri CLI:

```powershell
cargo install tauri-cli --version "^2"
.\Build-Release.ps1 -Bundle
```

Atau langsung (frontend `ui/dist` harus sudah dibangun dengan `npm run build`):

```powershell
cd crates\sm-gui
cargo tauri build
```

Output installer berada di:

```
target\release\bundle\nsis\
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
- Upload artifact `service-manager-portable`

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
   New-Item -ItemType Directory -Force data | Out-Null
   Copy-Item services.example.json data\services.json
   Copy-Item projects.example.json data\projects.json
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

Config dibaca dari `<folder exe>\data`. Gunakan `SERVICE_MANAGER_HOME` untuk memakai folder config lain (misalnya saat development).

## Migrasi Config

- Aplikasi PowerShell lama sudah dihapus dari repositori; repositori ini hanya berisi implementasi Rust.
- Format `data/services.json` dan `data/projects.json` tetap versi 1.
- Backup config sebelum mengganti versi aplikasi.
- Jangan menjalankan dua instance yang menulis config yang sama secara bersamaan.
- Rollback: pulihkan config dari backup dan gunakan binary release Rust sebelumnya.

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
