# Service Manager RS

Implementasi bertahap Service Manager dalam Rust. Status saat ini: **Phase 0-5 selesai** (workspace, domain model, kompatibilitas config, SCM core, NSSM/native wrapper, import discovery, GUI Tauri, batch async, updater CLI, security hardening, test).

Rencana lengkap: [../docs/RUST-REWRITE-PLAN.md](../docs/RUST-REWRITE-PLAN.md).

## Struktur

```
service-manager-rs/
├─ Cargo.toml
└─ crates/
   ├─ sm-core/     # model, validasi, ordering, aturan update
   ├─ sm-config/   # services.json & projects.json, backup, atomic write
   ├─ sm-win/      # SCM, NSSM/native, discover/import, elevation, DPAPI
   ├─ sm-gui/      # Tauri GUI + batch async scheduler + log rotation
   └─ sm-updater/  # CLI updater Laravel + credential helper
```

## Build dan Test

```powershell
cd service-manager-rs
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# GUI
cargo run -p sm-gui
# dari target/debug, arahkan config ke folder aplikasi:
$env:SERVICE_MANAGER_HOME = "C:\tools\ServiceManager"

# Updater CLI
cargo run -p sm-updater -- list
cargo run -p sm-updater -- check --project kasirin
cargo run -p sm-updater -- update --project kasirin
```

Test default berjalan tanpa Administrator. Test yang mengubah service (start/stop/install/restart) membutuhkan Administrator dan dijalankan manual di server uji:

```powershell
cargo test -p sm-win -- --ignored
```

Test read-only memakai service bawaan `EventLog` untuk memverifikasi query status, sehingga tidak mengubah sistem.

## Kompatibilitas Config

- `services.json` versi 1: object `{ version, services: [...] }` atau array legacy.
- `projects.json` versi 1.
- Field tidak dikenal diabaikan; nilai enum tidak dikenal dinormalisasi (tipe → NSSM, startup → Auto).
- Config rusak dicadangkan ke `backup/services-broken-<timestamp>.json`.
- Backup otomatis maksimal 20 file, pola `services-<timestamp>.json`.

## Environment

| Variabel | Fungsi |
| --- | --- |
| `SERVICE_MANAGER_HOME` | Folder config/log/backup. Default: folder executable. |

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

## GUI Tauri (Phase 3)

`sm-gui` menyediakan UI setara WinForms: tabel service, form tambah/edit, import, backup/restore, log viewer, dan banner elevasi. Detail ada di [docs/RUST-GUI.md](../docs/RUST-GUI.md).

Batch Start/Stop/Restart berjalan di background:

- lapisan dependensi dari `sm-core::start_layers`;
- maksimal 4 operasi paralel;
- `spawn_blocking` untuk panggilan SCM;
- progress per service via event Tauri;
- tombol Cancel dengan penghentian kooperatif.

Test GUI mencakup scheduler batch (urutan dependensi, stop reversal, paralelisme, failure isolation, cancellation), state, dan log/export.

## Updater CLI (Phase 4)

`sm-updater` adalah implementasi Rust dari `ProjectUpdate.ps1`:

```powershell
sm-updater list
sm-updater check --project kasirin
sm-updater update --project kasirin
sm-updater update            # semua project enabled
```

Fitur: scan project Laravel, preflight remote, fetch, fast-forward merge, blokir migration, composer/npm/build/artisan kondisional, `postUpdateCommand`, log ganda (aplikasi + `storage/logs`), timeout, dan exit code. Detail ada di [docs/RUST-UPDATER.md](../docs/RUST-UPDATER.md).

## Security Hardening (Phase 5)

- `sm-win::dpapi` — enkripsi/dekripsi DPAPI dengan entropy opsional + test round-trip.
- `sm-core::redact_secrets` — redaksi token provider, Bearer, password=, dan userinfo URL.
- Log aplikasi GUI di-redaksi dan dirotasi otomatis pada 5 MB (`app.log.1`).
- `sm-updater credential store` — simpan PAT lewat Git Credential Manager (DPAPI), token dibaca tanpa echo dan tidak pernah ditulis ke log/file.
- Elevasi UAC via banner GUI dan `ShellExecuteW runas`.
- Detail: [docs/RUST-SECURITY.md](../docs/RUST-SECURITY.md).

## Status Fase

| Fase | Status |
| --- | --- |
| 0. Fondasi | Selesai |
| 1. Core SCM | Selesai |
| 2. NSSM & native | Selesai |
| 3. GUI | Selesai |
| 4. Updater | Selesai |
| 5. Security & hardening | Selesai |
| 6. Release | Belum |
