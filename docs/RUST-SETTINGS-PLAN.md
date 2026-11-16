# Rencana Settings, Tool Paths, dan Privileged Execution

Status: **Phase 1 selesai** (settings.json, backend, GUI Settings, tool detection, Scheduled Task management). Phase 2 (propagasi PHP/Apache, migrasi trigger) belum.

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
| Propagasi PHP/Apache + affected apps/services | Belum (Phase 2) |
| Migrasi trigger global ke settings.json | Belum (Phase 2) |
| `webServerService` per app | Belum (Phase 2) |
| Doctor lengkap, audit, log retention, dry-run | Belum (Phase 3) |
| Windows Service mode | Tidak diperlukan (no-login tidak dibutuhkan) |

Catatan implementasi Phase 1:

- Tool wajib: Git, PHP, Composer, npm. Tool lain opsional.
- `Password` logon type untuk Scheduled Task tidak ditawarkan di GUI karena installer
  membutuhkan credential; pilihan GUI adalah `Interactive` dan `S4U`. Tidak ada
  password task yang disimpan plaintext.
- `settings.json` belum otomatis dibuat saat startup; GUI menampilkan legacy fallback
  dan membuat file saat **Save Settings**.
- Trigger allowlist masih memakai `projects.json` sampai Phase 2.

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

- `variables` disiapkan untuk fase lanjutan; fase 1 hanya fixed paths.
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

- Variabel `${PHP_HOME}` dan environment expansion.
- Credential manager UI untuk PAT/service account.
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

- [ ] Migrasi settings lama + backup.
- [ ] Global trigger pindah ke settings.json.
- [ ] Perubahan Apache menampilkan affected services/apps.
- [ ] Apply Apache service baru setelah konfirmasi, dengan backup.
- [ ] `webServerService` opsional pada app.
- [ ] Effective PHP/Apache version tampil di daftar app.

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
