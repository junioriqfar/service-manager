# Service Manager GUI (Rust + Tauri)

Dokumen ini menjelaskan GUI utama Service Manager (Rust + Tauri + React/shadCN), termasuk cara menjalankan, fitur, dan perilaku batch async.

## Menjalankan

Frontend React perlu dibangun terlebih dahulu karena Tauri menyematkan `ui/dist`:

```powershell
Push-Location crates/sm-gui/ui
npm ci
npm run build
Pop-Location
cargo run -p sm-gui
```

Secara default config berada di `<folder exe>\data`. Saat dijalankan dari `target/debug`, aplikasi mencari ancestor yang berisi `Cargo.toml` + `data/`, sehingga memakai folder `data/` di root repo. Untuk folder lain, gunakan environment variable:

```powershell
$env:SERVICE_MANAGER_HOME = "C:\tools\ServiceManager"
cargo run -p sm-gui
```

Build debug menghasilkan `target\debug\sm-gui.exe`. Build release (frontend harus sudah dibangun, atau gunakan `Build-Release.ps1` yang menjalankan `npm ci`/`npm run build` otomatis):

```powershell
cargo build --release -p sm-gui
```

Paket portable (GUI + updater + trigger client + contoh config + docs):

```powershell
.\Build-Release.ps1
```

Installer NSIS dan CI dijelaskan di [RUST-RELEASE.md](RUST-RELEASE.md).

## Perilaku Jendela

- Jendela utama otomatis berada di tengah layar (`"center": true` di `tauri.conf.json`).
- Build **release** (`sm-gui.exe`) memakai subsystem Windows GUI, jadi **tidak** membuka cmd/terminal.
- Build **debug** (`cargo run -p sm-gui` atau `target/debug/sm-gui.exe`) tetap membuka console untuk log/panic.
- `sm-updater.exe` dan `sm-diag.exe` adalah aplikasi console dan memang menampilkan terminal saat dijalankan langsung.

## Fitur

- UI React + TypeScript (Vite, Tailwind CSS, shadCN UI) sepenuhnya dalam Bahasa Inggris.
- Tema terang/gelap/sistem dengan ikon Sun/Moon/Monitor; pilihan disimpan di `localStorage`, default mengikuti preferensi sistem.
- Tombol **Browse…** pada Application (file picker) dan Working Directory (folder picker).
- Klik kanan baris service menampilkan context menu: Edit, Install, Start, Stop, Restart, Open Log, Delete.
- Toolbar atas hanya berisi aksi global: Start All, Stop All, Restart All, Backup, Restore, Folder, App Log, About.
- Add, Import, dan Refresh dipindahkan ke header **Service List**; aksi per-service hanya lewat context menu.

- Tabel service: status, nama, display name, jenis, application, startup, restart.
- Form tambah/edit dengan field khusus Apache (`apacheRoot`, `apacheConfig`) dan PostgreSQL (`pgData`, `pgAccount`).
- Import service terpasang (NSSM/Apache/PostgreSQL) via enumerasi SCM.
- Log viewer: stdout + stderr, auto-refresh 2 detik, batas 128 KB dan 600 baris, tombol buka file.
- Backup config ke `services-export-<timestamp>.json`; restore dari file JSON.
- Log aplikasi (`logs/app.log`) di-redaksi dan dirotasi otomatis pada 5 MB (`app.log.1`).
- Banner peringatan bila tidak berjalan sebagai Administrator + tombol elevasi UAC.
- Status bar: jumlah terdaftar/terpasang/berjalan.
- Pola service global Cronical dan queue per project dijelaskan di [RUST-SERVICES.md](RUST-SERVICES.md).

## Tab Updater

Header memiliki tiga tab: **Services**, **Updater**, dan **Remote**.

- Tabel app registry: nama/ID, branch, toolchain, service lifecycle, status, last update.
- Tombol **Refresh**, **Add App**, **Check All**, **Update All**.
- Aksi per app: Run, Check, Test, Log, Edit, Delete.
- Form registrasi/edit: repo URL, local path (Browse…), branch, preset, package manager,
  composer/artisan/package-install/build, install/build command, queue restart,
  allowlist IP per app, service lifecycle, clone-on-first-run, health check, dan webhook.
- **Test** menjalankan `git rev-parse` + `git ls-remote`, lalu menampilkan preset dan
  package manager hasil deteksi.
- **Check All/Update All** berjalan di `spawn_blocking`, jadi UI tidak freeze.
- Dialog hasil menampilkan status per app; log terbaru tampil di panel bawah.
- Klik kanan baris app: Run, Check, Test, Log, Edit, Delete.
- Tooltip tersedia pada tab, tombol aksi, dan field penting.
- Tidak ada polling/auto-update. Aksi hanya dipicu tombol atau `sm-trigger` via SSH.
- Service lifecycle membutuhkan GUI/task elevated; tanpa Administrator, update dengan
  service akan gagal sebelum file diubah.

## Tab Remote

Tab **Remote** adalah GUI client untuk memicu `sm-trigger` lewat SSH dari mesin ini.

- Field: host, user, port, auth mode (key/password), identity file, password +
  remember (DPAPI), wrapper script, remote `sm-updater.exe`, remote `projects.json`
  opsional, dan app ID opsional.
- **Preview** menampilkan perintah `ssh` yang akan dijalankan.
- **Test SSH** menjalankan `echo sm-trigger-ok` dengan `BatchMode=yes`/`ConnectTimeout=10`
  untuk mode key, atau `SSH_ASKPASS` + `NumberOfPasswordPrompts=1` untuk mode password.
- **Check All / Update All** memicu wrapper/task di server.
- **Check App / Run App** untuk satu app.
- Output stdout/stderr di-stream lewat event Tauri `remote-trigger-output`.
- Konfigurasi disimpan di `data/trigger-client.json` (tidak di-commit). Password hanya
  disimpan jika **Remember password** aktif, dalam bentuk DPAPI ciphertext.

Tab ini bisa dipakai di PC client Windows tanpa perlu terminal. Untuk macOS/Linux,
gunakan CLI `sm-trigger`.

## Batch Async (Anti-Freeze)

Tombol **Start All / Stop All / Restart All** tidak berjalan di UI thread.

- Service diurutkan per lapisan dependensi (`start_layers`).
- Start memproses lapisan dari dependensi ke dependent; Stop membalik urutan lapisan; Restart memakai urutan start per service (stop lalu start).
- Maksimal 4 operasi paralel (`Semaphore`), sehingga service independen berjalan bersamaan.
- Setiap operasi berjalan di `tokio::task::spawn_blocking`; UI tetap merender dan menerima input.
- Progress dikirim via event Tauri `batch-started`, `batch-progress`, `batch-finished` dan ditampilkan per service.
- Tombol **Cancel** menghentikan penjadwalan service berikutnya. Operasi yang sedang berjalan berhenti secara kooperatif (polling 250 ms) atau setelah timeout 60 detik.
- Satu batch pada satu waktu; klik ganda ditolak dengan pesan "Another batch operation is already running."

## Struktur

| File | Fungsi |
| --- | --- |
| `crates/sm-gui/src/main.rs` | Entry point Tauri, registrasi command |
| `crates/sm-gui/src/commands.rs` | Command IPC (CRUD, install, start/stop, import, backup/restore, batch) |
| `crates/sm-gui/src/updater.rs` | Command IPC tab Updater (registry CRUD, test, run, log) |
| `crates/sm-gui/src/remote.rs` | Command IPC tab Remote (config, preview, test SSH, run streaming) |
| `crates/sm-gui/src/state.rs` | State aplikasi, service rows, status SCM, batch lock |
| `crates/sm-gui/src/batch.rs` | Scheduler batch async + test concurrency |
| `crates/sm-gui/src/util.rs` | Log tail, export, open path, app log |
| `crates/sm-gui/ui/` | Frontend React + TypeScript: source `ui/src`, build Vite `ui/dist` |
| `crates/sm-gui/capabilities/default.json` | Capability Tauri (`core:default`) |

## Pengujian

Frontend harus dibangun sebelum `cargo test`/`cargo build` karena `tauri.conf.json` memakai `ui/dist`:

```powershell
cd crates/sm-gui/ui
npm ci
npm run build
cd ../../..
cargo test -p sm-gui
cargo clippy -p sm-gui --all-targets -- -D warnings
```

Test GUI mencakup (total workspace 254 test):

- Urutan dan paralelisme batch (dependency, stop reversal, concurrency, failure isolation, cancellation).
- State: upsert/duplikat, simpan/muat config, batch lock.
- Util: log tail, batas baris, export JSON, app log.
- Updater: registry CRUD, state view, preservasi field lanjutan, validasi service.
- Remote: normalisasi config, save/load, preview wrapper/direct, BatchMode/askpass, DPAPI password.
- UI: label Bahasa Inggris, context menu, tooltip, tombol browse, toolbar global, tab Remote, dan ikon/tema.

Verifikasi runtime GUI dapat dilakukan dengan WebView2 remote debugging (opsional):

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
Start-Process .\target\debug\sm-gui.exe
# lalu buka http://127.0.0.1:9222/json atau gunakan DevTools
```

## Keterbatasan Saat Ini

- Application dan Working Directory sudah memakai file/folder picker; path stdout/stderr log masih berupa input teks.
- Installer MSI/NSIS dan code signing belum dibuat (fase rilis).
- Batch write operation membutuhkan Administrator; GUI menampilkan banner dan tombol elevasi.
- Rename service saat edit belum didukung (nama service dikunci).
