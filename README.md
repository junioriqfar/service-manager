# Service Manager

Aplikasi Windows untuk mengelola **Windows Service** (NSSM, Apache native, PostgreSQL native) dan **updater project Laravel**. Dibangun dengan **Rust + Tauri 2** dan frontend **React + TypeScript + Vite + Tailwind CSS + shadCN UI**. Aplikasi PowerShell lama sudah dihapus dari repositori.

> Jalankan GUI sebagai **Administrator** untuk install/start/stop/restart service Windows.

## Fitur

- Daftar service dengan status live dari Windows SCM.
- Tambah, edit, hapus, install, start, stop, dan restart service.
- Dukungan `nssm`, `apache` (`httpd -k`), dan `postgresql` (`pg_ctl register`).
- Import service NSSM/Apache/PostgreSQL yang sudah terpasang.
- Start All / Stop All / Restart All berjalan async di background, dengan progress per service dan tombol Cancel.
- Startup `auto`, `delayed`, `manual`; restart on crash + delay.
- Log viewer (stdout/stderr, auto-refresh), backup/restore config.
- UI Bahasa Inggris, tema **light/dark/system**, icon Lucide, file/folder picker, dan context menu klik kanan.
- Updater Laravel `sm-updater`: cek/update Git, blokir migration, composer/npm/build/artisan, credential helper.
- Diagnosa headless `sm-diag`.

## Komponen

| Binary | Fungsi |
| --- | --- |
| `sm-gui.exe` | GUI Tauri (release tanpa console, window center) |
| `sm-updater.exe` | Updater Laravel + credential helper (console) |
| `sm-diag.exe` | Diagnosa config, NSSM, admin, dan status service (console) |

## Kebutuhan

| Komponen | Keterangan |
| --- | --- |
| Windows | Windows 10/11 atau Windows Server x64 |
| WebView2 Runtime | Untuk GUI Tauri |
| NSSM | `C:\tools\nssm\nssm.exe` atau `..\nssm\nssm.exe` |
| Hak akses | Administrator untuk operasi service |
| Node.js 22+ | Hanya untuk build frontend |
| Rust stable + MSVC | Hanya untuk build dari source |
| Git/PHP/Composer/npm | Untuk fitur updater Laravel |

## Struktur Folder

```text
ServiceManager/
├─ Cargo.toml                   # workspace Rust
├─ Cargo.lock
├─ Build-Release.ps1
├─ crates/
│  ├─ sm-core/                  # model, validasi, ordering, update plan
│  ├─ sm-config/                # services.json, projects.json, backup
│  ├─ sm-win/                   # SCM, NSSM/native, discovery, DPAPI
│  ├─ sm-gui/                   # Tauri + React/shadCN UI
│  └─ sm-updater/               # updater Laravel + credential helper
├─ data/
│  ├─ services.json             # config runtime (tidak di-commit)
│  └─ projects.json             # config runtime (tidak di-commit)
├─ backup/                      # backup config (tidak di-commit)
├─ logs/                        # log aplikasi (tidak di-commit)
├─ dist/                        # hasil build portable (tidak di-commit)
├─ docs/                        # dokumentasi
├─ services.example.json        # contoh services.json
└─ projects.example.json        # contoh projects.json
```

## Mulai Cepat

1. Gunakan ZIP portable:
   ```powershell
   .\Build-Release.ps1
   # hasil: dist\ServiceManager-RS-<versi>.zip
   ```
2. Ekstrak, lalu salin contoh config ke folder `data`:
   ```powershell
   New-Item -ItemType Directory -Force .\data | Out-Null
   Copy-Item .\services.example.json .\data\services.json
   Copy-Item .\projects.example.json .\data\projects.json
   ```
3. Sesuaikan path service/project.
4. Jalankan `sm-gui.exe` sebagai Administrator.

Config dibaca dari `<folder exe>\data`. Untuk memakai folder lain:

```powershell
$env:SERVICE_MANAGER_HOME = "C:\tools\ServiceManager"
.\sm-gui.exe
```

## Build dari Source

```powershell
# Frontend wajib dibangun sebelum cargo build/test GUI
Push-Location crates/sm-gui/ui
npm ci
npm run build
Pop-Location

cargo build --release -p sm-gui -p sm-updater
```

Atau gunakan `Build-Release.ps1` yang menjalankan `npm ci`, `npm run build`, cargo build, dan membuat ZIP.

## Konfigurasi

- Skema `services.json`: [docs/CONFIGURATION.md](docs/CONFIGURATION.md)
- Pola scheduler global dan queue per project: [docs/RUST-SERVICES.md](docs/RUST-SERVICES.md)
- Config runtime (`data/services.json`, `data/projects.json`, `logs/`, `backup/`) **tidak di-commit** karena berisi path/internal server.
- Backup otomatis dan export manual semuanya disimpan di `backup/`.

## Service Global dan Queue

- **Cronical** dapat melayani beberapa project dalam satu service. Satu job per project di `cronical.dat`, masing-masing dengan `Home`.
- Service scheduler di config bernama **`GlobalCron`**; log stdout/stderr ada di `C:\laragon\bin\cronical\logs\cronical-service.log`.
- Service lama `KasirinCron` perlu dihentikan dan dihapus setelah `GlobalCron` terpasang (butuh Administrator).
- **Queue Laravel** memakai satu worker per project (Opsi A): `KasirinQueue`, `OpenPoQueue`, dan seterusnya, karena `queue:work` terikat pada codebase/database project.
- Supervisor multi-project dalam satu service (Opsi B) belum diimplementasikan.

Detail dan contoh config: [docs/RUST-SERVICES.md](docs/RUST-SERVICES.md).

## Updater Laravel

```powershell
sm-updater list
sm-updater check --project kasirin
sm-updater update --project kasirin
sm-updater update                    # semua project enabled
sm-updater credential store --help
```

Panduan lengkap: [docs/RUST-UPDATER.md](docs/RUST-UPDATER.md).

## Keamanan

- Kredensial Git disimpan lewat Git Credential Manager + DPAPI; token dibaca tanpa echo.
- DPAPI tersedia di `sm-win::dpapi`.
- Log aplikasi/updater di-redaksi dari pola token/password dan `app.log` dirotasi pada 5 MB.
- Config runtime dan kredensial tidak disimpan di repositori.
- Detail: [docs/RUST-SECURITY.md](docs/RUST-SECURITY.md).

## Testing

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Saat ini **180 unit test** lulus. Frontend harus dibangun terlebih dahulu (`npm ci && npm run build`) karena Tauri memakai `ui/dist`.

## Dokumentasi

| Dokumen | Isi |
| --- | --- |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | Skema `services.json`, tipe service, startup, backup |
| [docs/RUST-GUI.md](docs/RUST-GUI.md) | GUI, tema, batch async, build frontend |
| [docs/RUST-SERVICES.md](docs/RUST-SERVICES.md) | Cronical multi-project, queue per project |
| [docs/RUST-UPDATER.md](docs/RUST-UPDATER.md) | Panduan updater Laravel |
| [docs/RUST-UPDATER-PLAN.md](docs/RUST-UPDATER-PLAN.md) | Inventaris fitur, backlog, dan roadmap updater |
| [docs/RUST-UPDATER-REGISTRY-PLAN.md](docs/RUST-UPDATER-REGISTRY-PLAN.md) | Registry, toolchain, service lifecycle, SSH trigger, IP whitelist |
| [docs/RUST-SECURITY.md](docs/RUST-SECURITY.md) | DPAPI, kredensial, redaksi log |
| [docs/RUST-RELEASE.md](docs/RUST-RELEASE.md) | Build, installer, CI, deployment |
| [docs/RUST-REWRITE-PLAN.md](docs/RUST-REWRITE-PLAN.md) | Catatan historis migrasi ke Rust |
| [docs/RUST-README.md](docs/RUST-README.md) | Struktur crate dan status implementasi |

## Troubleshooting

| Masalah | Solusi |
| --- | --- |
| Service gagal install/start | Jalankan GUI sebagai Administrator dan cek banner elevasi. |
| NSSM tidak ditemukan | Letakkan `nssm.exe` di `C:\tools\nssm\nssm.exe` atau `..\nssm\nssm.exe`. |
| Config rusak | File lama dicadangkan ke `backup/services-broken-*.json`; perbaiki JSON lalu buka ulang. |
| `OpenPoQueue`/service baru stopped | Service harus dibuat/diterapkan sebagai Administrator; gunakan **Edit → Save** lalu **Start**. |
| `GlobalCron` Not Installed | Jalankan app sebagai Administrator, Install `GlobalCron`, Start, lalu hapus service lama `KasirinCron`. |
| Cloudflared log error ke `127.0.0.1` | Itu error origin aplikasi (mis. queue/API), bukan config Service Manager; cek service/port origin. |
| Updater gagal akses remote | Simpan kredensial dengan `sm-updater credential store`; cek PAT/akun. |
| Update diblokir migration | Jalankan migration manual, lalu ulangi update. |
| GUI tidak muncul | Pastikan WebView2 Runtime terpasang dan jalankan sebagai Administrator. |

## Lisensi

Belum ditentukan.
