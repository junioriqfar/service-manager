# Rencana Registry + Trigger Update

Dokumen ini merancang evolusi `sm-updater` dari **scan folder + update manual** menjadi **registry aplikasi + update yang dipicu dari PC 143**. Fokusnya bukan polling setiap menit, tetapi satu trigger aman dari komputer Anda ke server 160.

- Status: **rencana / belum diimplementasikan**
- Basis: `sm-updater` saat ini (37 unit test, E2E Git lokal)
- Prinsip: trigger eksplisit, aman, idempoten, tanpa polling default
- Config target: `data/projects.json` versi 2 (kompatibel versi 1)

---

## 1. Koreksi Alur Lama

Alur lama **bukan polling** dan bukan Task Scheduler. Yang terjadi:

1. Anda menjalankan trigger dari PC 143.
2. Trigger masuk ke server 160 melalui **SSH**.
3. `ProjectUpdate.ps1` membaca environment `SSH_CONNECTION` atau `SSH_CLIENT`.
4. IP client dibandingkan dengan `allowedClientIPs` di `projects.json`.
5. Jika diizinkan, script melakukan cek/fetch/update satu kali lalu selesai.

Jadi sifatnya **event-driven / manual-triggered**, bukan background loop. Otomatisasi yang dimaksud adalah otomatisasi langkah setelah trigger: `git pull`, Composer, npm/build, dan queue restart.

## 2. Tujuan

1. Aplikasi harus **didaftarkan** dulu: repo URL, local path, branch, dan policy update.
2. Anda memicu update dari PC 143 (SSH) atau dari GUI server.
3. Server mengecek semua app terdaftar dan hanya mengupdate yang berubah.
4. Tidak ada polling setiap menit secara default.
5. Trigger dari IP yang tidak diizinkan ditolak.
6. Tetap aman: working tree kotor/branch salah/migration baru tidak diupdate otomatis.
7. Status dan log dapat dilihat dari CLI/GUI.

## 3. Perbedaan dengan Kondisi Saat Ini

| Aspek | Sekarang | Target |
| --- | --- | --- |
| Penentuan project | Scan `scanRoots` | Registry aplikasi eksplisit |
| Repo URL | Tidak disimpan | Disimpan per app |
| Trigger | SSH manual ke script PowerShell | SSH manual ke `sm-updater run --all` (atau GUI) |
| Polling | Tidak ada | **Tetap tidak ada secara default** |
| IP allowlist | `SSH_CONNECTION`/`SSH_CLIENT` | Dipertahankan di Rust |
| Update | Manual satu project/all | Satu trigger untuk semua app terdaftar |
| Status | Log per run | State persisten + riwayat |
| Ganti service | `queue:restart` | `queue:restart` + service restart opsional |
| GUI | Belum ada | Tab Updater, tombol Update All |

## 4. Arsitektur Target

```
PC 143 (client)
   │  ssh EGOV22@10.255.240.160
   │  sm-updater run --all
   ▼
Server 160
   ├─ validasi IP SSH (allowedClientIPs)
   ├─ baca registry data/projects.json
   ├─ lock per app
   ├─ untuk tiap app: cek Git → update jika berubah
   ├─ composer/npm/build/artisan/queue restart
   └─ tulis state + log
```

Tidak ada daemon/polling pada fase awal. Trigger bisa berupa:

- SSH remote command dari PC 143 (sama seperti sekarang),
- menjalankan `sm-updater` langsung di server,
- tombol **Update All** di GUI (nanti).

## 5. Schema Registry (`data/projects.json` versi 2)

Versi 1 tetap dibaca. `scanRoots`/`projects` lama dapat dimigrasikan menjadi `apps`.

```json
{
  "version": 2,
  "settings": {
    "gitPath": "C:\\laragon\\bin\\git\\cmd\\git.exe",
    "phpPath": "C:\\laragon\\bin\\php\\php.exe",
    "composerPath": "C:\\laragon\\bin\\composer\\composer.bat",
    "npmPath": "C:\\Program Files\\nodejs\\npm.cmd",
    "logDir": "C:\\tools\\ServiceManager\\logs",
    "allowedClientIPs": ["10.255.240.143", "127.0.0.1", "::1"],
    "maxConcurrentUpdates": 1
  },
  "apps": [
    {
      "id": "kasirin",
      "name": "Kasirin",
      "enabled": true,
      "repoUrl": "https://github.com/example/kasirin.git",
      "localPath": "D:\\Projects\\PHPProjects\\kasirin",
      "branch": "main",
      "credentialProfile": "github-junioriqfar",
      "cloneOnFirstRun": false,
      "composer": "auto",
      "build": "auto",
      "queueRestart": true,
      "migrationPolicy": "block",
      "preUpdateCommand": "",
      "postUpdateCommand": "",
      "restartServices": [],
      "healthCheck": {
        "enabled": false,
        "url": "",
        "timeoutSeconds": 30
      },
      "notify": {
        "webhookUrl": "",
        "on": ["failed", "updated"]
      }
    }
  ],
  "scheduler": {
    "enabled": false,
    "checkIntervalMinutes": 5
  }
}
```

### 5.1 Field Wajib Registrasi

| Field | Wajib | Keterangan |
| --- | --- | --- |
| `id` | Ya | Slug unik |
| `name` | Ya | Nama tampilan |
| `repoUrl` | Ya | HTTPS/SSH, tanpa token di URL |
| `localPath` | Ya | Folder project atau target clone |
| `branch` | Ya | Branch yang dipantau |
| `enabled` | Ya | Ikut trigger atau tidak |
| `composer`/`build` | Tidak | `auto`, `always`, `never` |
| `queueRestart` | Tidak | Restart queue setelah update |

### 5.2 Field Trigger

| Field | Default | Keterangan |
| --- | --- | --- |
| `allowedClientIPs` | `["127.0.0.1","::1"]` | IP SSH yang boleh memicu |
| `scheduler.enabled` | `false` | Polling hanya jika diaktifkan |
| `scheduler.checkIntervalMinutes` | `5` | Hanya dipakai jika scheduler aktif |
| `maxConcurrentUpdates` | `1` | Batas update paralel (jika banyak app) |

`autoUpdate` per app tidak diperlukan pada fase trigger. App yang `enabled` akan diproses saat `run --all`. Mode `check` tetap tersedia untuk dry-run.

## 6. Fitur yang Akan Dibuat

### 6.1 Registrasi Aplikasi

| Fitur | Deskripsi |
| --- | --- |
| Add app (CLI) | `sm-updater app add --id ... --repo ... --path ... --branch ...` |
| Add app (GUI) | Form di tab Updater |
| Edit/Remove/Enable | Kelola registry tanpa mengubah folder project |
| Test connection | `git ls-remote`, validasi path/branch/Laravel |
| Clone on first run | Clone repo jika `localPath` belum ada |
| Import dari scan | Ubah project hasil scan menjadi registry entry |
| Validasi | ID unik, URL bersih, path tidak konflik |

### 6.2 Trigger dari PC 143

| Fitur | Deskripsi |
| --- | --- |
| SSH trigger | `sm-updater run --all` dari PC 143 |
| IP allowlist | Baca `SSH_CONNECTION`/`SSH_CLIENT`, cocokkan `allowedClientIPs` |
| Local trigger | Tanpa env SSH (langsung di server) diizinkan secara default |
| Unauthorized | Exit code khusus (mis. `3`) + pesan jelas |
| Dry-run | `sm-updater run --all --check` |
| Single app | `sm-updater run --app kasirin` |
| Lock | Cegah dua trigger berjalan bersamaan |
| No-op | App tanpa perubahan tidak diubah |

### 6.3 Pipeline Update

Pipeline lama dipertahankan:

1. Validasi repo/path/branch.
2. Cek working tree bersih.
3. Preflight auth remote.
4. Fetch + fast-forward.
5. Blokir migration.
6. Composer jika perlu.
7. npm ci/install + build jika perlu.
8. `php artisan optimize:clear`.
9. `php artisan queue:restart`.
10. Pre/post command.
11. Restart service terpilih (opsional).
12. Health check (opsional).
13. Simpan state + log + rollback hint.

### 6.4 State, Riwayat, dan Lock

File runtime: `data/updater-state.json` dan `data/updater.lock` (tidak di-commit).

Status per app:

- `idle`
- `checking`
- `updating`
- `up-to-date`
- `blocked-migration`
- `dirty`
- `failed`

Data yang disimpan:

- `lastCheckedAt`, `lastUpdatedAt`, `lastCommit`, `lastError`, `durationMs`.
- Riwayat update (JSONL/log).
- Lock: PID, timestamp, app; stale lock timeout.

### 6.5 CLI

```powershell
# Registry
sm-updater app add --id kasirin --name Kasirin --repo https://... --path D:\... --branch main
sm-updater app list
sm-updater app show kasirin
sm-updater app edit kasirin --branch develop
sm-updater app remove kasirin
sm-updater app test kasirin

# Trigger (dari PC 143 atau server)
sm-updater run --all
sm-updater run --all --check
sm-updater run --app kasirin
sm-updater run --app kasirin --check

# Status/log
sm-updater status
sm-updater logs kasirin
```

Kompatibilitas: `list`, `check`, `update`, `--project`, `--all` tetap bekerja.

Contoh trigger SSH dari PC 143:

```powershell
ssh EGOV22@10.255.240.160 "C:\tools\ServiceManager\target\release\sm-updater.exe run --all"
```

### 6.6 GUI (Tab Updater)

- Tab **Services** dan **Updater**.
- Tabel app terdaftar: name, repo, branch, status, last check, last update.
- Tombol: Add, Edit, Remove, Test, **Update All**, Check All, Refresh.
- Form registrasi: repo URL, local path, branch, composer/build, queue restart, service restart, hooks.
- Status badge + log.
- Tidak ada background polling; semua aksi dipicu tombol/user.

### 6.7 Opsional (Fase Lanjutan)

- `scheduler.enabled=true` + `run --due` via Task Scheduler.
- Windows Service `GlobalUpdater` (`daemon`).
- Webhook GitHub/GitLab (real-time, butuh endpoint publik).
- Notifikasi webhook dan health check.
- Rollback command/otomatis.
- Update window.

## 7. Alur Trigger

```
PC 143
  └─ ssh server160 "sm-updater run --all"
       ├─ cek SSH client IP
       ├─ baca registry
       ├─ lock global/per app
       ├─ untuk tiap app enabled:
       │    ├─ validasi repo/path/branch
       │    ├─ git ls-remote/fetch
       │    ├─ jika tidak berubah → up-to-date
       │    ├─ migration? → blocked-migration
       │    ├─ fast-forward merge
       │    ├─ composer/npm/build/artisan
       │    ├─ queue:restart / restartServices
       │    └─ simpan state + log
       └─ ringkasan + exit code
```

## 8. Keamanan

- IP allowlist dari `SSH_CONNECTION`/`SSH_CLIENT` (sama seperti perilaku lama).
- `repoUrl` tidak boleh berisi kredensial.
- Token tetap di GCM/DPAPI; config hanya menyimpan profil/host.
- Trigger lokal tanpa SSH diizinkan untuk admin server.
- `pre/postUpdateCommand` hanya dari config admin.
- Lock mencegah trigger ganda.
- Migration tetap manual.
- Log di-redaksi.

## 9. Migrasi Config

- v1 `projects.json` tetap dibaca.
- `sm-updater migrate-config` mengubah `projects`/`scanRoots` menjadi `apps`.
- Backup otomatis ke `backup/` sebelum menulis.
- App tanpa `repoUrl` tetap bisa update manual, tetapi tidak masuk registry sampai dilengkapi.

## 10. Fase Implementasi

| Fase | Deliverable | Estimasi |
| --- | --- | --- |
| A. Registry & trigger | Schema v2, migrasi, `app add/list/edit/remove/test`, `run --all`, IP allowlist, lock, test | 4-6 hari |
| B. GUI Updater | Tab, tabel, form registrasi, test, Update All/Check All, status/log | 5-7 hari |
| C. Integrasi | Restart service, pre/post hook, health check, webhook opsional | 3-5 hari |
| D. Opsional | Task Scheduler/polling, daemon, rollback | 3-5 hari |

**Total fase inti:** 12-18 person-days; fitur opsional 3-5 hari tambahan.

## 11. Kriteria Penerimaan

- [ ] App harus terdaftar sebelum diproses `run --all`.
- [ ] Trigger dari PC 143 diizinkan hanya jika IP ada di `allowedClientIPs`.
- [ ] Trigger dari IP lain ditolak dengan exit code jelas.
- [ ] Trigger tidak melakukan polling; hanya satu kali cek/update.
- [ ] App tanpa perubahan tidak diubah.
- [ ] Working tree kotor/branch salah/migration baru tidak diupdate otomatis.
- [ ] Satu trigger memproses semua app enabled dengan lock.
- [ ] State/log per app tersedia di CLI dan GUI.
- [ ] Config v1 tetap terbaca; migrasi v2 aman + backup.
- [ ] Tidak ada token/password di config/log.

## 12. Non-Goals

- Polling setiap menit secara default.
- Auto-run migration.
- Merge conflict resolution otomatis.
- Menyimpan token di config.
- Deploy non-Laravel pada fase awal.

## 13. Pertanyaan Terbuka

1. Trigger dari PC 143 tetap via **SSH remote command**, atau ada aplikasi/shortcut khusus?
2. Apakah trigger harus bisa dijalankan tanpa login RDP (SSH key) — seperti sekarang?
3. Apakah GUI Updater perlu tombol **Update All** atau cukup CLI/SSH?
4. Apakah `allowedClientIPs` tetap dipakai, atau mau ditambah token/secret?
5. Apakah clone repo otomatis diperlukan untuk app baru?
6. Apakah service queue perlu direstart selain `php artisan queue:restart`?

## 14. Rekomendasi

1. **Pertahankan model trigger SSH** dari PC 143; jangan bangun polling di fase awal.
2. Bangun registry + `sm-updater run --all` + IP allowlist + lock sebagai fitur inti.
3. Tambahkan GUI tab Updater setelah CLI stabil.
4. Polling/daemon/webhook hanya jika nanti benar-benar dibutuhkan.
5. Default app baru: `enabled: true`, tetapi update hanya berjalan saat trigger.

---

## Referensi

- Implementasi saat ini: [RUST-UPDATER.md](RUST-UPDATER.md)
- Inventaris fitur: [RUST-UPDATER-PLAN.md](RUST-UPDATER-PLAN.md)
- Service global/queue: [RUST-SERVICES.md](RUST-SERVICES.md)
- Keamanan: [RUST-SECURITY.md](RUST-SECURITY.md)
