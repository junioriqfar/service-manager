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

Header memiliki empat tab: **Services**, **Updater**, **Remote**, dan **Settings**.

- Tabel app registry: nama/ID, branch, toolchain, service lifecycle, status, last update.
  Untuk preset Laravel ditampilkan versi PHP efektif; app dengan `webServerService`
  menampilkan nama service Apache dan versi Apache dari settings efektif.
- Tombol **Refresh**, **Add App**, **Check All**, **Update All**.
- Aksi per app: Preview Plan, Run, Check, Test, Log, History, Edit, Delete.
- Form registrasi/edit: repo URL, local path (Browse…), branch, preset, package manager,
  composer/artisan/package-install/build, install/build command, queue restart,
  web server Apache opsional, allowlist IP per app, service lifecycle, clone-on-first-run,
  health check, dan webhook.
- Pilihan **Web server (Apache)** hanya berisi service `serviceType=apache` dari
  `data/services.json`; kosong berarti app tidak terhubung eksplisit ke Apache.
- **Test** menjalankan `git rev-parse` + `git ls-remote`, lalu menampilkan preset dan
  package manager hasil deteksi.
- **Preview Plan** (dry run) memanggil `plan_updater_app` dan menampilkan dialog:
  badge status (`up-to-date`, `dirty`, `blocked-migration`, `ready`,
  `clone-required`, `error`), branch/HEAD/remote HEAD, incoming commit, changed
  files, migration blocker, command per group (install/build/artisan), service
  stop/restart/start + state running, health check, webhook (redacted), dan
  warnings. Daftar panjang memakai area scroll dengan tinggi maksimum.
- Dialog plan menampilkan catatan bahwa plan hanya fetch remote refs dan
  **does not merge, build, install, or change services**. Loading state terlihat
  saat fetch/inspeksi berjalan; error ditampilkan di dialog.
- **Check All/Update All** berjalan di `spawn_blocking`, jadi UI tidak freeze.
- Dialog hasil menampilkan status per app; log terbaru tampil di panel bawah.
- **History** (context menu, setelah Log) memanggil `load_updater_history` dan
  menampilkan 50 entry terbaru per app: timestamp, badge status, pesan, commit
  pendek, durasi, dan client IP trigger. Dialog punya state loading/error serta empty
  state **No history yet.**, dan area daftar bisa di-scroll. History bersifat
  append-only dan tidak dihapus oleh **Clean Logs Now**.
- Klik kanan baris app: Preview Plan, Run, Check, Test, Log, History, Edit, Delete.
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

## Tab Settings

Tab **Settings** mengelola konfigurasi mesin di `data/settings.json`.

- **Toolchain**: kartu untuk Git, PHP, Composer, Node, npm, pnpm, yarn, Bun, Go,
  Apache, dan NSSM. Setiap kartu memiliki path, **Browse…**, **Auto-detect**,
  **Test**, dan badge versi/status.
- **Auto-detect** mencari path yang sedang diisi, `PATH`, dan lokasi umum
  (Laragon PHP/Apache, Program Files Node/Go, `%USERPROFILE%\.bun`,
  `C:\tools\nssm`). Apache juga mengisi `root` dan `config` bila ditemukan.
- **Test** menjalankan versi tool (`git --version`, `php -v`, `composer --version`,
  `node --version`, `npm --version`, `pnpm --version`, `yarn --version`,
  `bun --version`, `go version`, `httpd -v`). NSSM hanya diverifikasi keberadaan
  filenya karena `nssm version` membuka GUI dan tidak pernah selesai. File `.bat`/`.cmd`
  dijalankan melalui `cmd.exe /c` oleh `SystemRunner`.
- **Updater defaults**: log directory, max concurrent updates, timeout
  preflight/fetch/tool/clone, dan log retention days. Tombol **Clean Logs Now** di
  samping retention days membuka konfirmasi
  "Delete logs older than N days? Active/recent logs are skipped." lalu memanggil
  `prune_logs_now`. Setelah selesai, toast dan teks inline menampilkan jumlah file,
  byte yang dihapus, file aktif yang dilewati, dan jumlah error. Retention hanya
  manual (tidak berjalan saat update), tidak butuh Administrator, menghapus
  `project-update-*.log`/`updater-task-*.log` yang lebih lama dari N hari (file
  dalam 5 menit terakhir selalu dilewati), dan tidak menghapus history atau audit
  Apache. Nilai 0 berarti retention dinonaktifkan dan tombol delete menghasilkan
  error yang jelas.
- **Trigger**: editor allowlist IP/CIDR (dipisah koma atau newline), switch
  **Allow local trigger**, IP client saat ini dari `SSH_CONNECTION`/`SSH_CLIENT`,
  dan tombol **Migrate Trigger**. Migrasi hanya muncul saat `settings.json` belum ada
  atau trigger `projects.json` berbeda; migrasi menyalin trigger ke `settings.json`
  tanpa mengubah `projects.json`.
- **Privileged Execution**: status Scheduled Task (`installed`, state, run level,
  user, logon type, last run, last result), field task name/user/logon type, dan
  tombol **Install Task**/**Repair Task**, **Remove Task**, **Test Task**.
  Install/remove membutuhkan Administrator dan memanggil
  `scripts/Install-UpdaterTask.ps1` atau `Unregister-ScheduledTask`; test memanggil
  `scripts/Invoke-UpdaterTask.ps1 -Check`.
- **Diagnostics**: tool wajib yang kosong, path tool yang tidak ditemukan,
  Scheduled Task yang belum terpasang, error status task, dan notice **Legacy
  fallback** saat `settings.json` belum ada.
- **Doctor**: laporan kesiapan read-only; lihat bagian Doctor di bawah.
- Saat `settings.json` belum ada, tab menampilkan nilai efektif dari
  `projects.json` dan **tidak** menulis file sampai **Save Settings** atau
  **Migrate Trigger** diklik.
- Tidak ada polling. Data dimuat saat tab dibuka atau **Reload** diklik; perubahan
  hanya disimpan lewat **Save Settings** dan hasilnya ditampilkan sebagai toast.

### Doctor (readiness)

Kartu **Doctor** di tab Settings menjawab pertanyaan "apakah server ini siap
di-update?" tanpa mengubah apa pun.

- **Run Doctor** memanggil command `run_doctor`. Backend berjalan di
  `spawn_blocking` dan mengembalikan `DoctorReport` berisi `generatedAt`,
  `overall`, `checks`, `counts` (ok/warning/error/skipped), dan `basePath`.
- Pemeriksaan dikelompokkan per kategori:
  - **Environment**: status Administrator.
  - **Tools**: Git/PHP/Composer/npm wajib; pnpm/yarn/Bun/Go/Apache/NSSM opsional,
    memakai path efektif `settings.json` dan version test singkat. NSSM hanya
    diverifikasi keberadaan filenya karena tidak punya CLI version yang aman.
  - **Settings**: `settings.json` ada/valid, legacy fallback, Apache root/config.
  - **Registry**: `projects.json`, app enabled + `localPath`, `webServerService`.
  - **Services**: `services.json` dan status SCM service Apache yang direferensikan.
  - **Remote access**: `git ls-remote --heads` per app, timeout pendek, tanpa prompt
    credential; URL userinfo di-redaksi.
  - **Scheduled Task**: installed, state, dan last result.
  - **SSH**: service `sshd` dan port 22, best effort.
  - **Storage**: `data/`, `logs/`, `backup/` writable dan disk bebas.
  - **Pending changes**: `data/apache-pending.json`.
  - **PHP references**: path `php.exe` hardcoded yang bukan `${PHP}`.
- Badge overall: **Healthy** (semua ok/skipped), **Warning** (ada warning tanpa
  error), **Error** (ada minimal satu error), **Checking** saat berjalan.
- Badge per check: **OK**, **Warning**, **Error**, **Skipped**. `Skipped` tidak
  pernah menaikkan overall.
- **Copy Report** menyalin laporan plain text (overall, counts, generatedAt,
  basePath, check + detail + hint) ke clipboard dan menampilkan toast. Jika
  clipboard tidak tersedia, toast error ditampilkan.
- Doctor read-only: tidak menulis `settings.json`, `projects.json`,
  `services.json`, atau pending Apache dan tidak start/stop service. Satu-satunya
  write adalah file probe sementara di folder storage yang sudah ada, langsung
  dihapus; folder yang belum ada tidak dibuat.
- Detail dan hint di-redaksi dengan `sm_core::redact_secrets`. Network/credential
  remote yang gagal hanya warning karena bisa transien.

### Apache change preview/apply

Saat **Save Settings** mengubah `tools.apache.path`, `root`, `config`, atau `version`:

- Settings tetap disimpan lebih dulu (`settings.json` lama dibackup ke
  `backup/settings-<timestamp>.json`).
- Tab menampilkan panel **Apache change pending** dengan path/version/root/config
  lama → baru, tabel **Affected services**, daftar **Affected apps**, dan peringatan
  Administrator.
- Preview disimpan di `data/apache-pending.json` (tidak di-commit), sehingga tetap
  muncul setelah tab ditutup atau aplikasi dibuka ulang. **Later** hanya menutup
  detail preview; pending tetap ada.
- **Apply Now** memanggil `apply_apache_change`: backup `services.json`, update
  `executable`/`apacheRoot`/`apacheConfig`, update Apache service `binPath`, stop
  service yang berjalan, apply SCM, lalu start kembali. Jika satu langkah gagal,
  `services.json` lama dipulihkan, config SCM lama dicoba dipasang ulang, service
  yang tadi berjalan dicoba start, dan pending dipertahankan.
- Tanpa Administrator, tombol Apply disabled dan command tetap menolak apply;
  settings + pending tidak hilang. Panel menampilkan error dari backend.
- Setelah sukses, pending dihapus dan hasil apply menampilkan service serta path
  audit `logs/apache-apply.log`. Reload memanggil `apache_change_status` untuk
  menampilkan banner pending yang belum di-apply.
- Affected apps pada preview perubahan Apache tetap hanya berasal dari
  `apps[].webServerService`. Saran otomatis dari DocumentRoot tersedia terpisah di
  kartu **Apache App Mapping** dan tidak pernah ditulis tanpa konfirmasi.

### Apache App Mapping

Kartu **Apache App Mapping** di tab Settings menghubungkan app ke service Apache
tanpa mengedit `projects.json` secara manual:

- **Detect Apps** menjalankan `detect_apache_mappings`. Bila ada beberapa service
  `serviceType=apache`, pilih service lebih dulu; konfigurasi yang dipakai adalah
  `apacheRoot` + `apacheConfig` service tersebut (fallback ke settings Apache).
- Parser membaca config secara read-only: `DocumentRoot`, `ServerRoot`, `Define`,
  `${VAR}`, `Include`/`IncludeOptional`, wildcard (mis. `sites-enabled/*.conf`),
  dan include bersarang. Batas kedalaman 12, 200 file, serta visited set.
- Suggestion menampilkan app, `localPath` → `documentRoot`, file config sumber,
  jenis match (`exact`, `descendant`, `ancestor`), dan confidence (`high`/`medium`).
  Exact/descendant bernilai high; app yang localPath-nya berada di bawah
  DocumentRoot bernilai medium.
- App yang sudah memiliki `webServerService` muncul di bagian **Already mapped**
  dengan checkbox disabled dan nama service lama. Mapping lama tidak ditimpa.
- Pilih checkbox app yang diinginkan lalu klik **Apply Mappings**
  (`apply_apache_mappings`). Command memvalidasi service Apache, membackup
  `backup/projects-<timestamp>.json`, lalu hanya menulis `webServerService` untuk
  app terpilih dan menyimpan `projects.json` secara atomik.
- Warning parser (include wajib hilang, variabel tidak dikenal, batas traversal)
  ditampilkan di kartu. Deteksi tidak pernah mengubah file config atau registry.

### PHP hardcoded references dan variabel command

Saat **Save Settings** mengubah `tools.php.path`:

- Backend membandingkan PHP path/root efektif lama dan baru. Jika masih ada
  command app yang memuat path/root lama, hasil save membawa `phpPathChange`
  berisi app, field, dan value terkait. `settings.json` tetap tersimpan.
- Field yang dipindai: `installCommand`, `buildCommand`, `preUpdateCommand`,
  `postUpdateCommand`, serta `commands.install/build/preUpdate/postUpdate`
  (`program`, `args[]`, `cwd`, `shell`, dan value `env`). Field yang sudah memakai
  `${PHP}`/`${PHP_HOME}` dilewati. Pencocokan Windows case-insensitive dan
  menganggap `/` sama dengan `\`.
- GUI menampilkan dialog **PHP references found** dengan old/new PHP path/root,
  app/field/value, dan tombol:
  - **Use ${PHP}** — memanggil `replace_php_references` mode `variable`, mengganti
    path lama dengan `${PHP}` dan root lama dengan `${PHP_HOME}`;
  - **Use new path** — memanggil command yang sama mode `new_path` dengan path/root
    baru;
  - **Leave** — menutup dialog tanpa mengubah `projects.json`.
- Replace hanya menyentuh app yang direferensikan, membackup
  `backup/projects-<timestamp>.json`, dan menyimpan `projects.json` atomik.
  Tidak ada rewrite otomatis saat Save maupun saat scan.
- Built-in variable yang tersedia untuk command: `${PHP}`, `${PHP_HOME}`,
  `${GIT}`, `${COMPOSER}`, `${NPM}`, `${BUN}`, `${GO}`, `${APACHE}`,
  `${APACHE_HOME}`, `${APACHE_CONFIG}`. Variabel admin dari
  `settings.json.variables` juga tersedia; nama built-in menang.
- Ekspansi rekursif maksimal 8 level. `${NAME}` yang tidak dikenal dibiarkan,
  environment variable tidak diekspansi, dan tidak ada pembacaan file repo.

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
| `crates/sm-gui/src/updater.rs` | Command IPC tab Updater (registry CRUD, test, run, log, plan read-only) |
| `crates/sm-gui/src/remote.rs` | Command IPC tab Remote (config, preview, test SSH, run streaming) |
| `crates/sm-gui/src/settings.rs` | Command IPC tab Settings (toolchain detection/test, settings save/load, trigger migration, Scheduled Task) |
| `crates/sm-gui/src/doctor.rs` | Doctor read-only: status/check/report types, agregasi overall, check environment/tools/settings/registry/services/remote/task/SSH/storage/pending/PHP, command `run_doctor` |
| `crates/sm-gui/src/apache.rs` | Pending Apache change, matching service/app, apply/rollback/audit, command `apache_change_status`/`apply_apache_change` |
| `crates/sm-gui/src/apache_config.rs` | Parser DocumentRoot/include/variabel, saran mapping, command `detect_apache_mappings`/`apply_apache_mappings` |
| `crates/sm-gui/src/php_paths.rs` | Scan referensi PHP hardcoded, dialog payload `phpPathChange`, command `replace_php_references` (variable/new_path, backup, selected apps) |
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

Test GUI mencakup (total workspace 413 test):

- Urutan dan paralelisme batch (dependency, stop reversal, concurrency, failure isolation, cancellation).
- State: upsert/duplikat, simpan/muat config, batch lock.
- Util: log tail, batas baris, export JSON, app log.
- Updater: registry CRUD, state view, preservasi field lanjutan, validasi service,
  validasi `webServerService` (Apache vs non-Apache), versi PHP/Apache efektif,
  command `plan_updater_app` (plan ready tanpa perubahan service), dan
  `load_updater_history` (filter case-insensitive + default limit 50).
- Retention: `prune_logs_at` menghapus log updater/project lama, menolak
  `days = 0` dengan pesan jelas, dan `prune_logs_now_at` memakai `logDir` efektif
  dari `settings.json`.
- Plan: status up-to-date/ready/dirty/blocked-migration/clone-required/error,
  command per group, service read-only, redaksi webhook, dan smoke CLI.
- Remote: normalisasi config, save/load, preview wrapper/direct, BatchMode/askpass, DPAPI password.
- Settings: parser JSON status Scheduled Task, validasi nama task/user/logon type,
  fallback settings lama, save/load helper, migrasi trigger eksplisit, diagnostik,
  deteksi/resolusi path tool, Apache root/config, dan test versi melalui file `.bat`.
- Apache propagation: matching service berdasarkan executable/root, first config
  semua service Apache, filter app via `webServerService`, pending
  save/load/clear/validation, update hanya service terdampak, backup services,
  audit log, dan penolakan apply tanpa elevation.
- Apache DocumentRoot: `Define`/`${VAR}`, include wildcard + nested, relative
  path/ServerRoot, batas variabel, matching exact/descendant/ancestor, deduplikasi,
  already mapped, apply parsial + backup projects, dan penolakan service salah.
- PHP references: ekspansi `${PHP}`/`${PHP_HOME}`/variabel lain, scan
  install/build/pre/post serta `commands.*` (program/args/cwd/shell/env), skip
  `${PHP}`, normalisasi case/slash, replace mode variable/new_path hanya untuk app
  terpilih, backup `projects.json`, dan ada/tidaknya `phpPathChange` setelah save.
- Doctor: agregasi overall (error > warning > ok, skipped tidak menaikkan),
  counts, required vs optional tool (kosong/invalid/gagal versi), settings
  legacy/invalid, Apache root/config dengan temp dir, registry missing/invalid,
  `localPath`, `webServerService` Apache/non-Apache, services invalid, service SCM
  terpasang/tidak, task status, pending Apache, SSH, storage probe temp dir,
  disk threshold, remote sukses/gagal, scan PHP hardcoded, redaksi detail/hint,
  dan report builder.
- UI: label Bahasa Inggris, context menu, tooltip, tombol browse, toolbar global, tab Remote,
  tab Settings (termasuk kartu Trigger, Apache App Mapping, field web server,
  panel Apache pending, dialog PHP references, kartu Doctor dengan Run/Copy, dan
  tombol Clean Logs Now), dialog History dengan empty state, serta ikon/tema.

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
- Tab Settings: logon type `Password` belum didukung dari GUI (gunakan `Interactive`
  atau `S4U`), dan nama Scheduled Task tidak boleh memakai folder path.
- Sprint 2.1–2.4 sudah menyediakan trigger efektif, migrasi trigger eksplisit,
  tampilan versi PHP/Apache per app, preview/apply Apache explicit dengan pending,
  backup, rollback, dan audit, saran `webServerService` dari DocumentRoot Apache,
  serta variabel `${PHP}`/`${PHP_HOME}` dan dialog replace referensi PHP hardcoded.
  Replace tetap hanya berjalan setelah user memilih **Use ${PHP}** atau
  **Use new path**.
