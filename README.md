# Service Manager

Aplikasi Windows untuk mengelola **Windows Service** (NSSM, Apache native, PostgreSQL native) dan **updater project multi-toolchain** (Laravel, Node, Bun, Go, static, custom). Dibangun dengan **Rust + Tauri 2** dan frontend **React + TypeScript + Vite + Tailwind CSS + shadCN UI**. Aplikasi PowerShell lama sudah dihapus dari repositori.

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
- Updater `sm-updater`: registry app, trigger SSH dengan allowlist IP/CIDR, cek/update Git, blokir migration, toolchain Composer/npm/pnpm/yarn/Bun/Go/static/custom, service lifecycle, credential helper.
- Client `sm-trigger` cross-platform (Windows/macOS/Linux) untuk memicu update via SSH tanpa polling.
- GUI tab **Remote** di `sm-gui` untuk trigger via SSH dengan output live (Windows client), mendukung SSH key/agent atau password (DPAPI).
- Tooltip pada aksi utama dan context menu klik kanan di daftar app Updater: Run, Check, Test, Log, Edit, Delete.
- GUI tab **Updater**: registry CRUD, test, Check All/Update All, status, log, serta tampilan versi PHP/Apache efektif dan `webServerService`.
- GUI tab **Settings**: path toolchain dengan Browse/Auto-detect/Test, default updater, trigger allowlist + Migrate Trigger, status dan Install/Repair/Remove/Test Scheduled Task, serta diagnostik.
- **Doctor** read-only di Settings: cek Administrator, tool, settings/registry, service Apache + SCM, remote Git, Scheduled Task, SSH, storage/disk, pending Apache, dan referensi PHP; hasil dikelompokkan dengan badge status dan bisa disalin lewat **Copy Report**.
- Kartu **Apache App Mapping** mem-parsing DocumentRoot/Include/variabel config Apache dan menyarankan `webServerService`; mapping hanya ditulis setelah app dipilih dan **Apply Mappings** diklik (dengan backup `projects.json`).
- Command app mendukung variabel `${PHP}`, `${PHP_HOME}`, `${GIT}`, `${APACHE_HOME}`, dan variabel settings lain. Jika path PHP berubah, GUI menampilkan dialog referensi PHP hardcoded dengan pilihan **Use ${PHP}**, **Use new path**, atau **Leave**; tidak ada rewrite otomatis.
- Perubahan path/root/config Apache menampilkan preview service/app terdampak, disimpan sebagai pending, lalu di-apply eksplisit (Administrator) ke `services.json` + Windows SCM dengan backup, rollback, dan audit.
- Wrapper elevated `scripts/Invoke-UpdaterTask.ps1` + Scheduled Task `ServiceManager-Updater`.
- Diagnosa headless `sm-diag`.

## Komponen

| Binary | Fungsi |
| --- | --- |
| `sm-gui.exe` | GUI Tauri (release tanpa console, window center) |
| `sm-updater.exe` | Updater registry + trigger + credential helper (console) |
| `sm-trigger.exe` | Client SSH cross-platform untuk memicu updater |
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
| Git/PHP/Composer/npm/Bun/Go | Untuk fitur updater multi-toolchain (sesuai app yang didaftarkan) |
| OpenSSH client | Untuk `sm-trigger` di sisi client |

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
│  ├─ sm-gui/                   # Tauri + React/shadCN UI (Services/Updater/Remote/Settings)
│  ├─ sm-trigger/               # client SSH cross-platform
│  └─ sm-updater/               # registry updater + trigger + credential helper
├─ scripts/                     # wrapper Scheduled Task elevated
├─ data/
│  ├─ services.json             # config runtime (tidak di-commit)
│  ├─ projects.json             # config runtime (tidak di-commit)
│  ├─ settings.json             # settings runtime (tidak di-commit)
│  └─ apache-pending.json       # pending apply Apache (tidak di-commit)
├─ backup/                      # backup config (tidak di-commit)
├─ logs/                        # log aplikasi (tidak di-commit)
├─ dist/                        # hasil build portable (tidak di-commit)
├─ docs/                        # dokumentasi
├─ services.example.json        # contoh services.json
├─ projects.example.json        # contoh projects.json
└─ settings.example.json        # contoh settings.json
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
5. Di PC client, buka tab **Remote** dan isi host/user/SSH key atau password/wrapper
   untuk trigger dari jarak jauh, atau gunakan `sm-trigger.exe` di terminal.

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

cargo build --release -p sm-gui -p sm-updater -p sm-trigger
```

Atau gunakan `Build-Release.ps1` yang menjalankan `npm ci`, `npm run build`, cargo build, dan membuat ZIP.

## Konfigurasi

- Skema `services.json`: [docs/CONFIGURATION.md](docs/CONFIGURATION.md)
- Pola scheduler global dan queue per project: [docs/RUST-SERVICES.md](docs/RUST-SERVICES.md)
- Config runtime (`data/services.json`, `data/projects.json`, `data/settings.json`, `data/apache-pending.json`, `logs/`, `backup/`) **tidak di-commit** karena berisi path/internal server.
- Backup otomatis dan export manual semuanya disimpan di `backup/`.

## Service Global dan Queue

- **Cronical** dapat melayani beberapa project dalam satu service. Satu job per project di `cronical.dat`, masing-masing dengan `Home`.
- Service scheduler di config bernama **`GlobalCron`**; log stdout/stderr ada di `C:\laragon\bin\cronical\logs\cronical-service.log`.
- Service lama `KasirinCron` perlu dihentikan dan dihapus setelah `GlobalCron` terpasang (butuh Administrator).
- **Queue Laravel** memakai satu worker per project (Opsi A): `KasirinQueue`, `OpenPoQueue`, dan seterusnya, karena `queue:work` terikat pada codebase/database project.
- Supervisor multi-project dalam satu service (Opsi B) belum diimplementasikan.

Detail dan contoh config: [docs/RUST-SERVICES.md](docs/RUST-SERVICES.md).

## Updater Registry

```powershell
# Registrasi app
sm-updater app add --id kasirin --name Kasirin `
  --repo https://github.com/example/kasirin.git `
  --path D:\Projects\PHPProjects\kasirin --branch main `
  --preset laravel --package-manager npm `
  --stop-before-build kasirin --restart-after kasirin

sm-updater app list
sm-updater app show kasirin
sm-updater app edit kasirin --branch develop
sm-updater app test kasirin

# Trigger manual (bukan polling)
sm-updater run --all
sm-updater run --all --check
sm-updater run --app kasirin
sm-updater status
sm-updater logs kasirin

# Trigger dari client lain (Windows/macOS/Linux)
sm-trigger --host 10.255.240.160 --user EGOV22 --all --check
sm-trigger --host 10.255.240.160 --user EGOV22 --all `
  --wrapper "C:\tools\ServiceManager\scripts\Invoke-UpdaterTask.ps1"

# Mode lama tetap tersedia
sm-updater list
sm-updater check --project kasirin
sm-updater update --project kasirin

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

Saat ini **375 unit test** workspace lulus. Frontend harus dibangun terlebih dahulu (`npm ci && npm run build`) karena Tauri memakai `ui/dist`.

## Dokumentasi

| Dokumen | Isi |
| --- | --- |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | Skema `services.json`, `projects.json`, `trigger-client.json`, backup |
| [docs/RUST-GUI.md](docs/RUST-GUI.md) | GUI Services/Updater/Remote/Settings, tema, batch async, build frontend |
| [docs/RUST-SERVICES.md](docs/RUST-SERVICES.md) | Cronical multi-project, queue per project |
| [docs/RUST-UPDATER.md](docs/RUST-UPDATER.md) | Panduan updater registry, toolchain, service lifecycle, trigger |
| [docs/RUST-UPDATER-PLAN.md](docs/RUST-UPDATER-PLAN.md) | Inventaris fitur, backlog, dan roadmap updater |
| [docs/RUST-UPDATER-REGISTRY-PLAN.md](docs/RUST-UPDATER-REGISTRY-PLAN.md) | Registry, toolchain, service lifecycle, SSH trigger, IP whitelist |
| [docs/RUST-TRIGGER.md](docs/RUST-TRIGGER.md) | Client SSH cross-platform dan wrapper Scheduled Task |
| [docs/RUST-SETTINGS-PLAN.md](docs/RUST-SETTINGS-PLAN.md) | Rencana settings.json, tool paths, propagasi PHP/Apache, privileged task |
| [docs/RUST-OPERATIONS-PLAN.md](docs/RUST-OPERATIONS-PLAN.md) | Rencana Fase 3: Doctor, dry-run plan, history/retention/audit |
| [docs/RUST-SECURITY.md](docs/RUST-SECURITY.md) | DPAPI, kredensial, redaksi log, SSH trigger, allowlist IP |
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
