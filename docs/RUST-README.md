# Service Manager RS

Implementasi Service Manager dalam Rust. Status: **Phase 0-7 selesai** (workspace, domain model, kompatibilitas config, SCM core, NSSM/native wrapper, import discovery, GUI Tauri, batch async, registry updater, toolchain multi-bahasa, service lifecycle, SSH trigger client, GUI Remote, security hardening, release & CI, test).

Rencana lengkap: [RUST-REWRITE-PLAN.md](RUST-REWRITE-PLAN.md).

## Struktur

```
ServiceManager/
├─ Cargo.toml
├─ Cargo.lock
├─ Build-Release.ps1
├─ crates/
│  ├─ sm-core/     # model, validasi, ordering, aturan update
│  ├─ sm-config/   # services.json & projects.json, data, backup, atomic write
│  ├─ sm-win/      # SCM, NSSM/native, discover/import, elevation, DPAPI
│  ├─ sm-gui/      # Tauri GUI (Services/Updater/Remote) + batch async + sm-diag
│  ├─ sm-trigger/  # client SSH cross-platform (CLI + library untuk GUI)
│  └─ sm-updater/  # registry updater, toolchain, service lifecycle, credential helper
├─ scripts/        # wrapper Scheduled Task elevated
├─ data/           # config runtime (services, projects, trigger-client, state)
├─ backup/         # semua backup config
├─ logs/           # log aplikasi/updater
└─ dist/           # hasil build portable
```

Binary:

| Binary | Fungsi |
| --- | --- |
| `sm-gui.exe` | GUI Tauri: Services, Updater, Remote Trigger |
| `sm-updater.exe` | Registry updater + credential helper (console) |
| `sm-trigger.exe` | Client SSH cross-platform untuk memicu updater |
| `sm-diag.exe` | Diagnosa headless (console) |

## Build dan Test

```powershell
# Frontend React (wajib sebelum cargo build/test GUI)
Push-Location crates/sm-gui/ui
npm ci
npm run build
Pop-Location

cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# GUI
cargo run -p sm-gui
# config runtime ada di data/; opsional override:
$env:SERVICE_MANAGER_HOME = "C:\tools\ServiceManager"

# Updater CLI
cargo run -p sm-updater -- app list
cargo run -p sm-updater -- run --all --check
cargo run -p sm-updater -- run --all
cargo run -p sm-updater -- status
cargo run -p sm-updater -- logs kasirin

# Trigger client
cargo run -p sm-trigger -- --host 10.255.240.160 --user EGOV22 --all --check
```

## Release

```powershell
# Portable ZIP (dist\ServiceManager-RS-<versi>.zip)
.\Build-Release.ps1

# Portable + installer NSIS (butuh cargo-tauri)
cargo install tauri-cli --version "^2"
.\Build-Release.ps1 -Bundle
```

Panduan lengkap: [RUST-RELEASE.md](RUST-RELEASE.md). CI: `.github/workflows/rust.yml` dan `release.yml`.

Test default berjalan tanpa Administrator. Test yang mengubah service (start/stop/install/restart) membutuhkan Administrator dan dijalankan manual di server uji:

```powershell
cargo test -p sm-win -- --ignored
```

Test read-only memakai service bawaan `EventLog` untuk memverifikasi query status, sehingga tidak mengubah sistem.

## Kompatibilitas Config

- `services.json` versi 1: object `{ version, services: [...] }` atau array legacy; disimpan di `data/services.json`.
- `projects.json` versi 2: registry `{ version: 2, settings, trigger, apps, scheduler }`; v1 tetap dibaca dan dimigrasikan di memori. Disimpan di `data/projects.json`.
- `trigger-client.json`: konfigurasi host/user SSH untuk tab Remote/GUI, disimpan di `data/trigger-client.json` (tidak di-commit).
- `updater-state.json`, `updater.lock`, `updater-request.json`, `updater-result.json`: state runtime updater.
- Field tidak dikenal diabaikan; nilai enum tidak dikenal dinormalisasi (tipe → NSSM, startup → Auto).
- Config rusak dicadangkan ke `backup/services-broken-<timestamp>.json`.
- Backup otomatis maksimal 20 file, pola `services-<timestamp>.json`.

## Environment

| Variabel | Fungsi |
| --- | --- |
| `SERVICE_MANAGER_HOME` | Folder config/log/backup. Default: folder executable (config di `data/`). |

## SCM Core (Phase 1)

Modul `sm-win::scm` menyediakan:

- `query_status` / `service_exists` — status numerik SCM (`Running`, `Stopped`, pending, dst.) dan PID.
- `start_service` / `stop_service` / `restart_service` — idempoten untuk kondisi sudah berjalan/berhenti.
- `wait_for_state` — polling dengan timeout, interval, dan `CancellationToken` berbasis `Arc<AtomicBool>`.
- `set_startup` / `set_delayed_start` — `ChangeServiceConfigW` + `ChangeServiceConfig2W`.
- `delete_service` — `DeleteService`, aman bila service sudah tidak ada.

Modul `sm-win::elevation` menyediakan cek token Administrator dan relaunch via `ShellExecuteW` (`runas`), termasuk quoting argumen ala `CommandToArgvW`.

## NSSM, Native, dan Import (Phase 2)

- `sm-win::nssm` — install/remove/apply config NSSM, termasuk `Application`, `AppParameters`, `AppDirectory`, log rotation, `AppExit`, restart delay, dependensi, dan recovery action `sc failure`.
- `sm-win::native` — install/uninstall Apache (`httpd -k install`) dan PostgreSQL (`pg_ctl register`), apply `sc config` untuk display/start/dependency, delayed start, dan failure actions.
- `sm-win::discovery` — enumerasi service Windows via `EnumServicesStatusExW` + `QueryServiceConfigW`, klasifikasi `nssm.exe`/`httpd.exe`/`pg_ctl.exe`, parsing command line (`-d`, `-f`, `-D`), pembacaan `PGDATA`, log Apache/PostgreSQL, dan nilai NSSM.
- `sm-win::process` — abstraksi `CommandRunner` (nyata dan mock) serta decoding output UTF-8/UTF-16LE.

Pengujian wrapper memakai `MockRunner`: tidak menjalankan NSSM/`sc.exe` sungguhan. Enumerasi read-only diuji terhadap service Windows yang ada.

## GUI Tauri (Phase 3 + 7)

`sm-gui` menyediakan UI React + shadCN dengan tiga tab: **Services**, **Updater**, dan **Remote**. Detail ada di [RUST-GUI.md](RUST-GUI.md).

- Tab **Services**: tabel service, form tambah/edit, import, backup/restore, log viewer.
- Tab **Updater**: registry app, toolchain, service lifecycle, test, Check/Update, log.
- Tab **Remote**: host/user/key/wrapper, Preview, Test SSH, Check/Update All, app tunggal, output live.
- UI sepenuhnya Bahasa Inggris, tema light/dark/system.
- Tombol **Browse…** membuka file picker (Application) dan folder picker (Working Directory).
- Toolbar atas hanya aksi global; Add/Import/Refresh ada di header Service List.
- Klik kanan baris service menampilkan context menu Edit/Install/Start/Stop/Restart/Open Log/Delete.
- Pola Cronical global dan queue per project ada di [RUST-SERVICES.md](RUST-SERVICES.md).

Batch Start/Stop/Restart berjalan di background:

- lapisan dependensi dari `sm-core::start_layers`;
- maksimal 4 operasi paralel;
- `spawn_blocking` untuk panggilan SCM;
- progress per service via event Tauri;
- tombol Cancel dengan penghentian kooperatif.

Test GUI mencakup scheduler batch (urutan dependensi, stop reversal, paralelisme, failure isolation, cancellation), state, log/export, registry updater, serta konfigurasi/preview Remote Trigger.

## Registry Updater (Phase 4 + 7)

`sm-updater` adalah engine registry multi-toolchain:

```powershell
sm-updater app add --id kasirin --repo https://... --path D:\... --branch main `
  --preset laravel --package-manager npm
sm-updater app list
sm-updater app test kasirin
sm-updater run --all --check
sm-updater run --all
sm-updater run --app kasirin
sm-updater status
sm-updater logs kasirin
```

Fitur: registry v2, migrasi v1, allowlist IP/CIDR global + per-app, lock, state, Git fast-forward, blokir migration, toolchain Composer/npm/pnpm/yarn/Bun/Go/static/custom, clone-on-first-run, service lifecycle (stop/restart/start), health check HTTP, webhook, dan credential helper. Mode lama `list/check/update --project` tetap tersedia. Panduan: [RUST-UPDATER.md](RUST-UPDATER.md). Roadmap: [RUST-UPDATER-PLAN.md](RUST-UPDATER-PLAN.md). Registry + trigger: [RUST-UPDATER-REGISTRY-PLAN.md](RUST-UPDATER-REGISTRY-PLAN.md). Client SSH: [RUST-TRIGGER.md](RUST-TRIGGER.md).

## Security Hardening (Phase 5)

- `sm-win::dpapi` — enkripsi/dekripsi DPAPI dengan entropy opsional + test round-trip.
- `sm-core::redact_secrets` — redaksi token provider, Bearer, password=, dan userinfo URL.
- Log aplikasi GUI di-redaksi dan dirotasi otomatis pada 5 MB (`app.log.1`).
- `sm-updater credential store` — simpan PAT lewat Git Credential Manager (DPAPI), token dibaca tanpa echo dan tidak pernah ditulis ke log/file.
- Elevasi UAC via banner GUI dan `ShellExecuteW runas`.
- SSH trigger: allowlist IP/CIDR dua lapis, `authorize` sebelum task elevated, Scheduled Task `RunLevel Highest`, request nonce + expiry 15 menit.
- GUI Remote memakai `BatchMode=yes` (key/agent saja) dan `ConnectTimeout=10`.
- Detail: [RUST-SECURITY.md](RUST-SECURITY.md).

## Status Fase

| Fase | Status |
| --- | --- |
| 0. Fondasi | Selesai |
| 1. Core SCM | Selesai |
| 2. NSSM & native | Selesai |
| 3. GUI | Selesai |
| 4. Updater | Selesai |
| 5. Security & hardening | Selesai |
| 6. Release | Selesai |
| 7. Registry + SSH trigger + GUI Remote | Selesai |
