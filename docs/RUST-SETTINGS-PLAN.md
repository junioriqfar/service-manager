# Rencana Settings, Tool Paths, dan Privileged Execution

Status: **Phase 1 selesai; Sprint 2.1–2.4 Phase 2 selesai; Doctor Sprint 3.1 selesai; Dry-run Plan Sprint 3.2 selesai; History/retention Sprint 3.3 selesai; Maintenance mode + export/load settings Sprint 3.4a selesai; Git credential UI + firewall read-only Sprint 3.4b selesai; Check All concurrency Sprint 3.5 stage 1 selesai** (fondasi trigger + webServerService + effective runtime + Apache explicit propagation + DocumentRoot suggestions + `${PHP}`/scan path PHP hardcoded + Doctor read-only + plan preview read-only + history/audit + maintenance/export + credential/firewall read-only + check-only parallel).

Dokumen ini menjawab revisi pengguna:

1. Setting global dipisah ke `data/settings.json`.
2. Perubahan path/versi Apache otomatis memperbarui daftar app/service yang memakai Apache.
3. Perubahan path/versi PHP otomatis berlaku untuk app Laravel.
4. Trigger hanya perlu berjalan saat user login; **tidak** membuat Windows Service privileged baru.
5. Password hanya boleh disimpan terenkripsi (DPAPI).
6. Pekerjaan dilakukan bertahap, bukan sekaligus.

---

## 0. Status Implementasi

| Bagian | Status |
| --- | --- |
| `data/settings.json` schema/load/save/migrasi fallback | Selesai |
| Tool path + deteksi + test versi | Selesai |
| GUI tab Settings | Selesai |
| Updater defaults | Selesai |
| Privileged Execution: status/install/repair/remove/test Scheduled Task | Selesai |
| Propagasi Apache + affected apps/services | Selesai (Sprint 2.2: preview, pending, apply, backup, rollback, audit) |
| Saran DocumentRoot → `webServerService` | Selesai (Sprint 2.3: parser config/includes/variabel, konfirmasi user) |
| Propagasi PHP hardcoded/`${PHP}` | Selesai (Sprint 2.4: variabel command + scan/replace eksplisit) |
| Migrasi trigger global ke settings.json | Selesai (Sprint 2.1, eksplisit) |
| `webServerService` per app | Selesai (Sprint 2.1) |
| Doctor read-only (readiness report) | Selesai (Sprint 3.1) |
| Dry-run plan preview | Selesai (Sprint 3.2) |
| History/retention/audit | Selesai (Sprint 3.3) |
| Maintenance mode + export/load settings | Selesai (Sprint 3.4a) |
| Git credential UI (store/test/clear) | Selesai (Sprint 3.4b) |
| Firewall SSH read-only | Selesai (Sprint 3.4b); add/remove rule ditunda |
| Check All concurrency (`maxConcurrentUpdates`) | Selesai (Sprint 3.5 stage 1: check-only; Run/Update tetap serial) |
| Backup/restore penuh (bukan hanya settings) | Belum (di luar sprint) |
| Windows Service mode | Tidak diperlukan (no-login tidak dibutuhkan) |

Catatan implementasi Phase 1:

- Tool wajib: Git, PHP, Composer, npm. Tool lain opsional.
- `Password` logon type untuk Scheduled Task tidak ditawarkan di GUI karena installer
  membutuhkan credential; pilihan GUI adalah `Interactive` dan `S4U`. Tidak ada
  password task yang disimpan plaintext.
- `settings.json` belum otomatis dibuat saat startup; GUI menampilkan legacy fallback
  dan membuat file saat **Save Settings**.
- Trigger allowlist sudah memakai `settings.json` bila ada; lihat catatan Sprint 2.1.

Catatan implementasi Sprint 2.1:

- `AppConfig.webServerService` opsional. Jika diisi, GUI memvalidasi bahwa nama service
  ada di `data/services.json` dan `serviceType=apache`.
- Trigger efektif: `settings.json.trigger` authoritative; `projects.json.trigger` hanya
  fallback saat `settings.json` belum ada. `sm-updater run/authorize` dan GUI memakai
  trigger efektif ini.
- `allowedClientIPs` divalidasi sebagai IP/CIDR IPv4 atau IPv6; entri kosong/invalid
  ditolak dengan pesan jelas.
- Migrasi trigger bersifat eksplisit lewat tombol **Migrate Trigger**. Command
  `migrate_trigger_to_settings` menyalin `projects.json.trigger` ke `settings.json`
  tanpa mengubah `projects.json`. Jika `settings.json` sudah ada, file lama dibackup
  ke `backup/settings-<timestamp>.json`.
- Daftar app Updater menampilkan versi PHP efektif (termasuk preset `auto` yang
  terdeteksi Laravel) dan service/versi Apache dari settings efektif.
- Belum termasuk Sprint 2.1: apply Apache (2.2), parser DocumentRoot (2.3), scan path
  PHP hardcoded/`${PHP}` (2.4).

Catatan implementasi Sprint 2.2:

- **Save Settings tidak langsung mengubah `services.json`.** Nilai Apache efektif lama
  dibaca lebih dulu; setelah settings disimpan, preview ditulis ke
  `data/apache-pending.json` (di-ignore git) bila path/root/config/version berubah.
- `settings.json` lama dibackup ke `backup/settings-<timestamp>.json` sebelum ditimpa.
  Backup pertama tidak dibuat saat file belum ada.
- Service Apache dicocokkan dengan executable lama, executable di bawah root lama,
  atau `apacheRoot` yang sama. Jika path lama kosong (konfigurasi pertama), semua
  service `serviceType=apache` masuk preview. App diambil hanya dari
  `apps[].webServerService` yang menunjuk service terdampak; tidak ada parser
  DocumentRoot.
- Tombol **Apply Now** membutuhkan Administrator. Tanpa elevation, settings dan
  pending tetap tersimpan dan command mengembalikan error.
- Apply menyimpan `services.json` (backup otomatis `backup/services-*.json`) sebelum
  menyentuh SCM, stop service yang sedang berjalan, memanggil `apply_service`,
  lalu start kembali service yang tadi berjalan. Timeout wait 60 detik.
- `sm-win` native apply untuk Apache ikut memperbarui `binPath` service menjadi
  `"httpd.exe" -d "<root>" -f "<config>" -k runservice`, sehingga path/root/config
  baru benar-benar dipakai SCM pada start berikutnya.
- Jika salah satu langkah gagal, `services.json` lama dipulihkan lewat `save_services`,
  config SCM lama dicoba dipasang ulang untuk service yang sudah ter-apply, service
  yang tadi berjalan dicoba di-start, dan pending tetap ada.
- Apply sukses menghapus pending dan menulis audit ke `logs/apache-apply.log`
  (timestamp, user, aksi, service, app, path/root/config/version lama-baru, hasil).
- Pending hanya dibuat jika path Apache baru menunjuk `httpd.exe`, `root` baru
  tidak kosong, dan minimal satu service yang cocok benar-benar berubah
  executable/root/config. Perubahan versi saja tidak meninggalkan pending no-op. Jika service berubah setelah preview, Apply ditolak dan meminta
  Save Settings ulang.
- Perubahan hanya `detectedAt` tidak memicu pending. Mengosongkan path Apache juga
  tidak menulis executable kosong ke service; propagation membutuhkan path baru.
- DocumentRoot dan `webServerService` otomatis (Sprint 2.3) serta path PHP
  hardcoded/`${PHP}` (Sprint 2.4) tetap di luar Sprint 2.2.

Catatan implementasi Sprint 2.3:

- `apache_config` parser read-only mendukung `Define`, `${VAR}`, `ServerRoot`,
  `Include`, `IncludeOptional`, wildcard include, dan include bersarang dengan batas
  kedalaman/file/ukuran.
- `detect_apache_mappings` hanya memberi saran. Mapping baru ditulis oleh
  `apply_apache_mappings` setelah user memilih app; `projects.json` dibackup ke
  `backup/projects-<timestamp>.json`.
- App yang sudah punya `webServerService` ditampilkan sebagai `alreadyMapped` dan
  tidak ditimpa otomatis.

Catatan implementasi Sprint 2.4:

- Built-in variabel command: `${PHP}`, `${PHP_HOME}`, `${GIT}`, `${COMPOSER}`,
  `${NPM}`, `${BUN}`, `${GO}`, `${APACHE}`, `${APACHE_HOME}`, `${APACHE_CONFIG}`.
  `settings.json.variables` menambah variabel admin; built-in menang jika nama sama.
- Ekspansi hanya untuk config admin, recursive maksimal 8 level, unknown placeholder
  dibiarkan, tidak membaca environment/repo.
- Perubahan path PHP memindai `installCommand`/`buildCommand`/`preUpdateCommand`/
  `postUpdateCommand` dan `commands.*`. User memilih **Use ${PHP}**, **Use new path**,
  atau **Leave**; tidak ada rewrite otomatis. `projects.json` dibackup sebelum replace.

Catatan implementasi Sprint 3.1 (Doctor):

- Kartu **Doctor** di tab Settings memanggil `run_doctor` dan menampilkan laporan
  read-only: overall, counts, grouped checks, detail, hint, generatedAt, basePath,
  serta tombol **Copy Report**.
- Check: Administrator; tool wajib/opsional + version test; `settings.json` dan
  legacy fallback; Apache root/config; `projects.json`, `localPath`, dan
  `webServerService`; `services.json` + status SCM service Apache; `git ls-remote`
  per app; Scheduled Task; `sshd`/port 22; storage writable + disk bebas; pending
  Apache; dan path PHP hardcoded.
- Status `ok`, `warning`, `error`, `skipped`; overall error jika ada error,
  warning jika ada warning, selain itu ok. `skipped` tidak menaikkan overall.
- Read-only: tidak menulis settings/projects/services/pending dan tidak
  start/stop service. Probe storage hanya membuat file sementara di folder yang
  sudah ada, lalu menghapusnya.
- Semua detail/hint di-redaksi dengan `sm_core::redact_secrets`.
- Batasan: check remote tidak interaktif (tidak bisa memasukkan credential saat
  Doctor berjalan), cek port SSH hanya `127.0.0.1:22` IPv4, dan Doctor tidak
  memperbaiki masalah otomatis.

Catatan implementasi Sprint 3.4a (maintenance + export/load):

- `settings.json.maintenance` (`enabled`, `message`) default nonaktif. Message
  di-trim, maksimal 500 karakter, dan menolak karakter kontrol.
- Trigger non-check ditolak sebelum app/Git/state/history saat maintenance aktif;
  `check` dan dry-run plan tetap berjalan. Doctor menampilkan check
  `maintenance.status` (warning saat aktif).
- GUI Settings memiliki kartu **Maintenance Mode**; tab Updater menampilkan banner
  peringatan yang dimuat lewat `load_maintenance_status` dan tidak punya shortcut
  disable.
- Helper `export_settings_at`/`load_settings_from_file_at` hanya memproses file
  `.json`. Export menulis settings efektif secara atomik tanpa memuat
  projects/services/log/history/rahasia. Load hanya mengganti draft GUI dan tidak
  menyimpan atau mengubah service/Apache.
- Command Tauri `export_settings_file` dan `load_settings_file` memakai
  `rfd::FileDialog`; tidak ada secret yang diekspor.
- Batasan: bukan backup penuh, bukan autentikasi, dan perubahan hasil load baru
  berlaku setelah **Save Settings**.

Catatan implementasi Sprint 3.4b (Git credentials + firewall read-only):

- Kredensial Git **tidak** menjadi bagian schema `settings.json`. Token disimpan
  Git Credential Manager dengan DPAPI; UI Settings hanya menyimpan host/username/app
  pilihan di state sementara. Store/test/clear memakai Git path efektif
  (`settings.json`, fallback legacy `projects.json.settings`).
- Host kredensial diekstrak dari `repoUrl` app dan didedup case-insensitive; app
  select untuk Test/Clear dibatasi ke app pada host terpilih, dengan fallback semua
  app bila host tidak punya app.
- Command baru: `list_git_credential_hosts`, `store_git_credential`,
  `test_git_credential`, `clear_git_credential`, `ssh_firewall_status`.
- Firewall SSH hanya read-only (`Get-NetFirewallRule` + port filter, timeout 20 detik)
  dan tidak menawarkan add/remove rule. Perubahan firewall tetap manual di Windows
  Firewall. Doctor tetap memeriksa port 22; **Test SSH** tetap di tab Remote.
- Token tidak pernah dikembalikan, ditampilkan ulang, ditulis ke settings/history/log,
  atau diekspor. Error credential di-redaksi.
- Batasan: firewall write ditunda, test remote membutuhkan app dengan `localPath` dan
  remote Git yang dapat diakses, dan snapshot firewall bisa unsupported bila cmdlet
  atau izin baca tidak tersedia.

## 1. Keputusan Arsitektur

### 1.1 `settings.json` terpisah dari `projects.json`

`data/settings.json` menjadi sumber utama untuk konfigurasi mesin:

- path toolchain: Git, PHP, Composer, Node, npm, pnpm, yarn, Bun, Go, Apache, NSSM;
- path runtime: log, backup;
- default updater: concurrency, timeout, retensi log;
- konfigurasi privileged execution (Scheduled Task);
- global trigger allowlist (dipindah bertahap);
- metadata deteksi versi.

`data/projects.json` tetap menjadi registry aplikasi:

- daftar `apps`;
- override per app;
- service lifecycle per app;
- allowlist per app.

### 1.2 Skema awal `settings.json`

```json
{
  "version": 1,
  "tools": {
    "git": {
      "path": "C:\\Program Files\\Git\\cmd\\git.exe",
      "version": "",
      "detectedAt": ""
    },
    "php": {
      "path": "C:\\laragon\\bin\\php\\php-8.3.16\\php.exe",
      "version": "8.3.16",
      "detectedAt": "2026-11-16T16:00:00+07:00"
    },
    "composer": {
      "path": "C:\\laragon\\bin\\composer\\composer.bat",
      "version": "",
      "detectedAt": ""
    },
    "node": {
      "path": "C:\\Program Files\\nodejs\\node.exe",
      "version": "",
      "detectedAt": ""
    },
    "npm": {
      "path": "C:\\Program Files\\nodejs\\npm.cmd",
      "version": "",
      "detectedAt": ""
    },
    "pnpm": { "path": "", "version": "", "detectedAt": "" },
    "yarn": { "path": "", "version": "", "detectedAt": "" },
    "bun": {
      "path": "C:\\Users\\EGOV22\\.bun\\bin\\bun.exe",
      "version": "",
      "detectedAt": ""
    },
    "go": { "path": "", "version": "", "detectedAt": "" },
    "apache": {
      "path": "C:\\laragon\\bin\\apache\\httpd-2.4.xx\\bin\\httpd.exe",
      "root": "C:\\laragon\\bin\\apache\\httpd-2.4.xx",
      "config": "conf/httpd.conf",
      "version": "",
      "detectedAt": ""
    },
    "nssm": {
      "path": "C:\\tools\\nssm\\nssm.exe",
      "version": "",
      "detectedAt": ""
    }
  },
  "updater": {
    "logDir": "C:\\tools\\ServiceManager\\logs",
    "maxConcurrentUpdates": 1,
    "timeouts": {
      "preflightSeconds": 30,
      "fetchSeconds": 120,
      "toolSeconds": 300,
      "cloneSeconds": 600
    },
    "logRetentionDays": 30
  },
  "trigger": {
    "allowedClientIPs": ["10.255.240.143", "127.0.0.1", "::1"],
    "allowLocal": true
  },
  "maintenance": {
    "enabled": false,
    "message": ""
  },
  "privilegedExecution": {
    "mode": "scheduled-task",
    "taskName": "ServiceManager-Updater",
    "userId": "EGOV22",
    "logonType": "Interactive"
  },
  "variables": {}
}
```

Catatan:

- `variables` menyimpan variabel admin untuk command; Sprint 2.4 menambahkan built-in `${PHP}` dan variabel tool lain (lihat bawah).
- Field `version`/`detectedAt` diisi oleh deteksi, bukan tangan user.
- Path boleh memakai environment variable Windows (`%ProgramFiles%`) pada fase 2.
- Password/token **tidak boleh** ada di file ini sebagai plaintext.

### 1.3 Urutan resolusi path

1. Override eksplisit per app (jika nanti tersedia).
2. `settings.json` `tools.<name>.path`.
3. PATH sistem / auto-detect.
4. Error dengan pesan yang menyebut Settings.

### 1.4 Migrasi dari konfigurasi lama

Saat `settings.json` belum ada:

1. Baca `projects.json.settings` (v2) sebagai fallback.
2. Petakan field lama:
   - `gitPath` → `tools.git.path`
   - `phpPath` → `tools.php.path`
   - `composerPath` → `tools.composer.path`
   - `npmPath` → `tools.npm.path`
   - `bunPath` → `tools.bun.path`
   - `pnpmPath` → `tools.pnpm.path`
   - `yarnPath` → `tools.yarn.path`
   - `goPath` → `tools.go.path`
   - `logDir` → `updater.logDir`
   - `maxConcurrentUpdates` → `updater.maxConcurrentUpdates`
3. `projects.json.trigger` → `settings.json.trigger` (fase 2), sambil tetap membaca fallback.
4. Tampilkan dialog **Migrate settings** di GUI; jangan menulis file baru secara diam-diam.
5. Backup `projects.json` sebelum migrasi.

`projects.json` tetap v2 pada fase 1. Field `settings`/`trigger` lama dibaca sebagai fallback
dan ditandai deprecated. Schema v3 dapat dipertimbangkan pada fase 2.

---

## 2. Propagasi PHP dan Apache

### 2.1 PHP

PHP dipakai oleh app Laravel untuk `php artisan ...` dan dapat dipakai custom command.

- `tools.php.path` adalah sumber global.
- Mengubah path PHP langsung berlaku untuk semua app Laravel pada eksekusi berikutnya.
- GUI menampilkan **effective PHP version** per app Laravel.
- Deteksi versi memakai `php.exe -v`.
- App yang memakai path PHP literal di custom command tidak otomatis berubah;
  fase 2 menyediakan variabel `${PHP}`/`${PHP_HOME}` agar command tidak hardcoded.

### 2.2 Apache

Apache adalah service Windows, bukan dependency langsung di `AppConfig` saat ini.

Perubahan `tools.apache.path`/`root` harus:

1. Menemukan service di `data/services.json` dengan `serviceType=apache`.
2. Mencocokkan `executable`/`apacheRoot` lama dengan path Apache lama.
3. Menampilkan daftar **affected services**.
4. Setelah konfirmasi admin:
   - backup `services.json`;
   - update `executable`, `apacheRoot`, dan `apacheConfig` bila relatif;
   - apply ulang config service jika terpasang (butuh Administrator).
5. Menampilkan app yang terkait dengan service tersebut.

Untuk menghubungkan app ke Apache, tambahkan field opsional pada app:

```json
"webServerService": "Apache2.4"
```

Tanpa field ini, sistem hanya bisa menampilkan affected **service**, bukan affected **app**.
Field ini juga dipakai untuk menampilkan badge/status “served by Apache2.4”.

### 2.3 Preview sebelum apply

Settings menampilkan:

- path lama → path baru;
- versi lama → versi baru;
- affected services;
- affected apps (jika `webServerService` diisi);
- peringatan bahwa service Apache akan di-apply ulang dan membutuhkan Administrator.

---

## 3. Privileged Execution: Task, Bukan Service Baru

### 3.1 Penjelasan dua jenis “service”

Ada dua hal berbeda:

1. **Service aplikasi** — Apache, PostgreSQL, queue worker, `opencode-quota`, dan
   `presensi-sidoarjo-server`. Ini sudah dikelola di tab **Services** melalui NSSM/native.
2. **Privileged runner untuk updater** — saat ini berupa **Scheduled Task**
   `ServiceManager-Updater`, bukan Windows Service. Task ini menjalankan updater
   dengan hak tertinggi saat dipicu.

Karena trigger hanya perlu saat user login, Windows Service privileged **tidak diperlukan**.
Menambah service terus-menerus elevated akan memperbesar attack surface dan menyulitkan
DPAPI/Git credentials.

### 3.2 Yang akan dibuat di Settings

Bagian **Privileged Execution**:

- status task: installed, ready, last run, last result;
- tombol **Install / Repair / Remove / Test**;
- user dan logon type (`Interactive` default);
- tombol **Test elevated trigger**;
- link ke `scripts/Install-UpdaterTask.ps1`;
- peringatan bahwa install/remove membutuhkan Administrator.

Install/remove task memakai script yang sudah ada; Settings hanya memanggilnya via
elevated process dengan konfirmasi.

### 3.3 Jika nanti butuh tanpa login

Baru pertimbangkan Windows Service dengan:

- named pipe + ACL ketat, bukan HTTP;
- hanya enum aksi (`run-all`, `run-app`, `check-all`), bukan command bebas;
- service account yang punya akses DPAPI/Git atau credential terpisah;
- password service disimpan DPAPI, tidak pernah plaintext/command line;
- audit log.

Itu bukan bagian fase 1.

---

## 4. Password dan Rahasia

- SSH password GUI: sudah DPAPI (Remember password).
- Service account password (jika service mode nanti): DPAPI, tidak plaintext.
- PAT Git: tetap GCM/DPAPI.
- Settings.json hanya menyimpan ciphertext/base64 atau referensi credential.
- Log tetap di-redaksi; password tidak masuk command line.

---

## 5. UI Settings (bertahap)

### Fase 1 — Toolchain dan privileged task

Tab **Settings**:

- **Toolchain**
  - kartu per tool: Git, PHP, Composer, Node, npm, pnpm, yarn, Bun, Go, Apache, NSSM;
  - path, Browse, Auto-detect, Test, versi, status;
  - tombol Save.
- **Updater defaults**
  - log dir, max concurrent, timeout, retention.
- **Privileged Execution**
  - status Scheduled Task, Install/Repair/Remove/Test.
- **Diagnostics**
  - readiness summary (tool hilang, task belum ada, config tidak valid).

### Fase 2 — Migrasi dan propagasi

- Migrate `projects.json.settings`/`trigger` → `settings.json`.
- Pindahkan global trigger allowlist.
- Preview affected PHP/Apache.
- Update Apache services otomatis setelah konfirmasi.
- Field `webServerService` pada app.
- Effective version per app.

### Fase 3 — Quality

- Doctor lengkap (versi, credential, firewall, permission, disk).
- Dry-run plan per app (commit, file, command, service).
- Log retention/cleanup.
- Audit log trigger (IP, aksi, hasil).
- Backup/restore settings.

### Fase 4 — Optional

- Environment expansion (`%VAR%`) untuk path admin; `${PHP_HOME}` dan variabel tool
  command sudah tersedia sejak Sprint 2.4.
- Credential manager UI untuk PAT/service account — **selesai 3.4b** (GCM/DPAPI,
  store/test/clear; tanpa token di settings/log).
- Windows Service mode jika benar-benar dibutuhkan.

---

## 6. API dan Modul

### Backend

- `crates/sm-updater/src/settings.rs`
  - load/save/validate/migrate;
  - `ToolPath`, `ToolKind`, `Settings`.
- `crates/sm-gui/src/settings.rs`
  - commands: `load_settings`, `save_settings`, `detect_tool`, `test_tool`,
    `migrate_settings`, `task_status`, `install_task`, `remove_task`, `test_task`.
- `crates/sm-gui/src/doctor.rs`
  - types: `DoctorStatus`, `DoctorCheck`, `DoctorCounts`, `DoctorReport`;
  - command: `run_doctor` (read-only, `spawn_blocking`);
  - helper: `run_doctor_at(ConfigPaths)` untuk report lengkap dari path config.
- Integrasi updater:
  - `UpdaterSettings` diganti/diadapter dari `Settings`;
  - toolchain resolver memakai path dari settings.json;
  - fallback `projects.json.settings` untuk kompatibilitas.

### Frontend

- `components/settings-view.tsx`
- `components/tool-path-card.tsx`
- `components/privileged-execution-card.tsx`
- tab **Settings** di `App.tsx`.

### File runtime

- `data/settings.json` (baru, tidak di-commit)
- `settings.example.json` (contoh, di-commit)
- `.gitignore` entry untuk `data/settings.json` dan `settings.json`.

---

## 7. Acceptance Criteria Fase 1

- [x] `settings.json` dibuat dan dibaca; `projects.json` lama tetap kompatibel.
- [x] Path Git/PHP/Composer/npm/Bun/Go/Apache/NSSM bisa diedit dari GUI.
- [x] Auto-detect dan Test menampilkan versi/error.
- [x] Updater memakai path dari settings.json.
- [x] Path PHP baru langsung dipakai app Laravel.
- [x] Scheduled Task status/install/repair/remove/test tersedia (admin).
- [x] Tidak ada password plaintext di settings.json/log.
- [x] Test unit + clippy + fmt hijau.
- [x] Dokumentasi diperbarui.

## 8. Acceptance Criteria Fase 2

- [x] Migrasi settings lama + backup (fallback in-memory; Save Settings/Migrate
  Trigger menulis settings.json dan membackup file lama).
- [x] Global trigger pindah ke settings.json (Sprint 2.1; migrasi eksplisit).
- [x] Perubahan Apache menampilkan affected services/apps (Sprint 2.2).
- [x] Apply Apache service baru setelah konfirmasi, dengan backup/rollback/audit
  (Sprint 2.2).
- [x] `webServerService` opsional pada app.
- [x] Effective PHP/Apache version tampil di daftar app.
- [x] Saran DocumentRoot → `webServerService` dengan konfirmasi user (Sprint 2.3).
- [x] `${PHP}`/`${PHP_HOME}` plus scan path PHP hardcoded dan replace eksplisit
  (Sprint 2.4; tidak ada rewrite otomatis).

---

## 9. Risiko

| Risiko | Mitigasi |
| --- | --- |
| Dua sumber settings | settings.json authoritative; fallback hanya saat file belum ada |
| Apache path berubah saat service jalan | Preview + konfirmasi + backup + apply setelah stop |
| DPAPI tidak tersedia untuk service account | Fase 1 tetap Scheduled Task; service mode ditunda |
| Password plaintext | DPAPI only; test memastikan file tidak memuat plaintext |
| Agent mengubah file bersamaan | Satu agent per fase; review dan test sebelum commit |
| Migrasi merusak config | Backup otomatis dan migrasi eksplisit, bukan diam-diam |

---

## 10. Pembagian Agent

| Agent | Tugas | Output |
| --- | --- | --- |
| Agent A — Backend Settings | schema settings.json, loader/saver, migrasi, tool detection/test, integrasi UpdaterSettings | kode + unit test |
| Agent B — GUI Settings | tab Settings, kartu tool, task management UI | kode + UI test |
| Agent C — Propagasi PHP/Apache | effective version, affected services/apps, apply Apache, `webServerService` | kode + test + docs |
| Agent D — Verifikasi | jalankan fmt/test/clippy, review diff, cek keamanan password | laporan verifikasi |

Urutan: **A → B → C → D**. Fase 1 hanya A+B+D; C pada fase 2.

---

## 11. Phase 2 Sprint Plan (disetujui)

Keputusan:

1. **Affected app Apache**: `webServerService` eksplisit + auto-detect DocumentRoot
   sebagai suggestion. Explicit menang.
2. **Apache apply**: preview dulu, lalu tombol **Apply** terpisah.
3. **Apply gagal**: settings baru tetap disimpan; `services.json` di-rollback;
   status `pending/error` ditampilkan.
4. **PHP**: path global; belum ada override per app.
5. **Migrasi trigger**: wizard terpisah + backup, bukan otomatis saat Save.
6. **`${PHP}`**: Sprint 2.4.

### Sprint 2.1 — Fondasi

Status: **Selesai**.

- `webServerService` opsional di app + validasi terhadap service Apache.
- Effective trigger dari `settings.json` (fallback `projects.json`).
- Settings UI trigger allowlist + allowLocal + Migrate Trigger.
- Effective PHP version/Apache service di tabel Updater.
- Tests + docs.

### Sprint 2.2 — Apache explicit propagation

Status: **Selesai**.

- Preview old/new Apache path/root/config disimpan di `data/apache-pending.json`.
- Affected services dicocokkan dari old executable/root; first config memilih semua
  service Apache.
- Affected apps dari `webServerService`.
- Apply: backup `settings.json`/`services.json`, update config, apply SCM termasuk
  Apache `binPath`, stop/start bila perlu, rollback saat gagal, audit log
  `logs/apache-apply.log`.
- Non-admin: settings disimpan, apply pending dan ditolak dengan pesan jelas.
- Tests: matching exe/root/first-config, filter app, pending roundtrip/validation,
  update hanya service terdampak, backup services, audit, non-elevation, UI labels.
- Batasan: belum ada DocumentRoot parser (2.3) atau `${PHP}` (2.4).

### Sprint 2.3 — DocumentRoot auto-detect

Status: **Selesai**.

- Parser read-only `httpd.conf` + `Include`/`IncludeOptional`, termasuk wildcard
  (`sites-enabled/*.conf`), `ServerRoot`, `Define`, dan ekspansi `${VAR}` rekursif.
- Batas traversal: kedalaman 12, 200 file, visited set, ukuran file 4 MB; tidak ada
  eksekusi Apache/command dan tidak ada network.
- Saran mapping `DocumentRoot` → `apps[].localPath` dengan exact/descendant bernilai
  high dan ancestor bernilai medium. App yang sudah punya `webServerService`
  dikembalikan sebagai `alreadyMapped` dan tidak ditimpa.
- Tab Settings memiliki kartu **Apache App Mapping** dengan pemilihan service
  (jika ada beberapa service Apache), **Detect Apps**, daftar checkbox saran,
  warnings, dan **Apply Mappings**.
- Apply hanya menulis `webServerService` untuk app yang dipilih, membackup
  `backup/projects-<timestamp>.json` lebih dulu, lalu menyimpan `projects.json`
  secara atomik. Tidak ada auto-write saat deteksi.
- Tests: `Define`/`${VAR}`, wildcard + nested include, relative path/ServerRoot,
  exact/descendant/ancestor, deduplikasi, already mapped, apply parsial + backup,
  penolakan service salah/hilang, dan UI source labels/commands.
- Batasan: parser tidak mengevaluasi `<IfModule>` atau blok kondisional lain; semua
  `DocumentRoot` yang terlihat dibaca. `${PHP}`/scan path PHP hardcoded tetap Sprint 2.4.

### Sprint 2.4 — PHP hardcoded path + `${PHP}`

Status: **Selesai**.

- `UpdaterSettings.variables` menyimpan built-in `PHP`, `PHP_HOME`, `GIT`,
  `COMPOSER`, `NPM`, `BUN`, `GO`, `APACHE`, `APACHE_HOME`, `APACHE_CONFIG`
  (plus variabel admin dari `settings.json.variables`). Nilai built-in selalu
  menang atas nama yang sama.
- `expand_variables` mengganti `${NAME}` secara rekursif maksimal 8 level;
  placeholder tidak dikenal dibiarkan, environment variable tidak dibaca, dan
  tidak ada akses file/repo.
- Ekspansi dipakai untuk `installCommand`, `buildCommand`, program/args/cwd/shell
  serta value `env` pada `commands.*`, dan `preUpdateCommand`/`postUpdateCommand`.
  Built-in tool path/args tidak diubah.
- Save Settings membandingkan PHP path efektif lama/baru. Jika berubah dan masih
  ada command app yang memuat path/root lama (case-insensitive, slash-insensitive),
  `SettingsSaveResult.phpPathChange` membawa daftar app/field/value. Tidak ada
  penulisan otomatis.
- Dialog GUI menampilkan old/new PHP path/root dan referensi. **Use ${PHP}**
  memanggil `replace_php_references` mode `variable` (path → `${PHP}`, root →
  `${PHP_HOME}`); **Use new path** memakai path/root baru; **Leave** hanya menutup.
- Replace hanya untuk app yang direferensikan, membackup
  `backup/projects-<timestamp>.json`, menyimpan `projects.json` atomik, lalu
  mengembalikan view updater terbaru. Nilai lain di string dipertahankan.
- Tests: ekspansi variabel (PHP/PHP_HOME/rekursif/unknown), custom command,
  install/build/pre/post, scan seluruh field command, skip `${PHP}`, case/slash
  normalization, replace variable/new_path, backup, selected apps, dan ada/tidaknya
  `phpPathChange`.
- Batasan: scan hanya field command app, bukan file project; replacement tidak
  dijalankan otomatis dan tidak menyentuh Apache.
