# Service Manager GUI (Rust + Tauri)

Dokumen ini menjelaskan GUI Rust yang menggantikan WinForms PowerShell, termasuk cara menjalankan, fitur, dan perilaku batch async.

## Menjalankan

```powershell
cd service-manager-rs
cargo run -p sm-gui
```

Secara default aplikasi membaca config dari folder executable (sama seperti `$PSScriptRoot` versi PowerShell). Untuk pengembangan dari `target/debug`, arahkan ke folder aplikasi:

```powershell
$env:SERVICE_MANAGER_HOME = "C:\tools\ServiceManager"
cargo run -p sm-gui
```

Build debug menghasilkan `service-manager-rs\target\debug\sm-gui.exe`. Build release:

```powershell
cargo build --release -p sm-gui
```

Paket portable (GUI + updater + contoh config + docs):

```powershell
cd service-manager-rs
.\Build-Release.ps1
```

Installer NSIS dan CI dijelaskan di [RUST-RELEASE.md](RUST-RELEASE.md).

## Fitur

- Tabel service: status, nama, display name, jenis, application, startup, restart.
- Toolbar: Tambah, Edit, Hapus, Install, Import, Refresh, Start/Stop/Restart, Start/Stop/Restart Semua, Backup, Restore, Folder, Log App, Tentang.
- Form tambah/edit dengan field khusus Apache (`apacheRoot`, `apacheConfig`) dan PostgreSQL (`pgData`, `pgAccount`).
- Import service terpasang (NSSM/Apache/PostgreSQL) via enumerasi SCM.
- Log viewer: stdout + stderr, auto-refresh 2 detik, batas 128 KB dan 600 baris, tombol buka file.
- Backup config ke `services-export-<timestamp>.json`; restore dari file JSON.
- Log aplikasi (`logs/app.log`) di-redaksi dan dirotasi otomatis pada 5 MB (`app.log.1`).
- Banner peringatan bila tidak berjalan sebagai Administrator + tombol elevasi UAC.
- Status bar: jumlah terdaftar/terpasang/berjalan.

## Batch Async (Anti-Freeze)

Tombol **Start Semua / Stop Semua / Restart Semua** tidak berjalan di UI thread.

- Service diurutkan per lapisan dependensi (`start_layers`).
- Start memproses lapisan dari dependensi ke dependent; Stop membalik urutan lapisan; Restart memakai urutan start per service (stop lalu start).
- Maksimal 4 operasi paralel (`Semaphore`), sehingga service independen berjalan bersamaan.
- Setiap operasi berjalan di `tokio::task::spawn_blocking`; UI tetap merender dan menerima input.
- Progress dikirim via event Tauri `batch-started`, `batch-progress`, `batch-finished` dan ditampilkan per service.
- Tombol **Batalkan** menghentikan penjadwalan service berikutnya. Operasi yang sedang berjalan berhenti secara kooperatif (polling 250 ms) atau setelah timeout 60 detik.
- Satu batch pada satu waktu; klik ganda ditolak dengan pesan "Operasi batch lain sedang berjalan."

## Struktur

| File | Fungsi |
| --- | --- |
| `crates/sm-gui/src/main.rs` | Entry point Tauri, registrasi command |
| `crates/sm-gui/src/commands.rs` | Command IPC (CRUD, install, start/stop, import, backup/restore, batch) |
| `crates/sm-gui/src/state.rs` | State aplikasi, service rows, status SCM, batch lock |
| `crates/sm-gui/src/batch.rs` | Scheduler batch async + test concurrency |
| `crates/sm-gui/src/util.rs` | Log tail, export, open path, app log |
| `crates/sm-gui/ui/` | HTML/CSS/JS (vanilla, tanpa npm) |
| `crates/sm-gui/capabilities/default.json` | Capability Tauri (`core:default`) |

## Pengujian

```powershell
cargo test -p sm-gui
cargo clippy -p sm-gui --all-targets -- -D warnings
```

Test GUI mencakup:

- Urutan dan paralelisme batch (dependency, stop reversal, concurrency, failure isolation, cancellation).
- State: upsert/duplikat, simpan/muat config, batch lock.
- Util: log tail, batas baris, export JSON, app log.

Verifikasi runtime GUI dapat dilakukan dengan WebView2 remote debugging (opsional):

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
Start-Process .\target\debug\sm-gui.exe
# lalu buka http://127.0.0.1:9222/json atau gunakan DevTools
```

## Keterbatasan Saat Ini

- Pemilihan file executable/log masih berupa input path (file picker ditambahkan pada fase berikutnya).
- Installer MSI/NSIS dan code signing belum dibuat (fase rilis).
- Batch write operation membutuhkan Administrator; GUI menampilkan banner dan tombol elevasi.
- Rename service saat edit belum didukung (nama service dikunci).
