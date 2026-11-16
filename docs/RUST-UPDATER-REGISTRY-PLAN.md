# Rencana Auto-Update Berbasis Registry

Dokumen ini merancang evolusi `sm-updater` dari **scan folder + update manual** menjadi **registry aplikasi + auto-update**. Aplikasi yang ingin diperbarui harus didaftarkan terlebih dahulu: alamat repo, path lokal, branch, dan kebijakan update.

- Status: **rencana / belum diimplementasikan**
- Basis: `sm-updater` saat ini (37 unit test, E2E Git lokal)
- Prinsip: aman, idempoten, bisa berjalan otomatis di server tanpa RDP ke PC kantor
- Config target: `data/projects.json` versi 2 (dengan kompatibilitas versi 1)

---

## 1. Tujuan

1. Aplikasi Laravel **didaftarkan eksplisit** sebelum bisa di-update otomatis.
2. Setiap aplikasi menyimpan **repo URL, local path, branch, dan policy update**.
3. Server memantau perubahan Git secara berkala dan menjalankan:
   - `git pull`/fast-forward,
   - Composer/npm/build/artisan bila perlu,
   - restart queue/service bila perlu.
4. Tidak perlu mengetik perintah berulang atau RDP ke PC kantor.
5. Tetap aman: tidak ada update jika working tree kotor, branch salah, atau ada migration baru.
6. Status dan log dapat dipantau dari CLI dan GUI.

## 2. Perbedaan dengan Kondisi Saat Ini

| Aspek | Saat ini | Target |
| --- | --- | --- |
| Penemuan project | Scan `scanRoots` + override | **Registry aplikasi** (utama) + scan sebagai import |
| Repo URL | Tidak disimpan | Wajib/opsional per app, dipakai untuk clone/validasi |
| Update | Manual `sm-updater update` | Otomatis oleh scheduler/daemon |
| Jadwal | Task Scheduler eksternal (dokumentasi) | Interval per app, update window, retry/backoff |
| Status | Log per run | State persisten + riwayat + status GUI |
| Credential | GCM/DPAPI akun yang menjalankan | Profil kredensial per host + strategi akun service |
| GUI | Belum ada | Tab **Updater**: daftar app, tambah/edit, check/update now, log |
| Service restart | `queue:restart` | `queue:restart` + daftar service yang direstart (opsional) |

## 3. Arsitektur Target

```
┌─────────────────────────────────────────────────────────────┐
│ sm-gui (tab Services | tab Updater)                         │
│ - daftar app, form registrasi, status, check/update now     │
└───────────────────────┬─────────────────────────────────────┘
                        │ Tauri commands
┌───────────────────────▼─────────────────────────────────────┐
│ sm-updater (library + CLI + scheduler)                      │
│ - registry config, validation, state, lock, polling         │
│ - git/composer/npm/artisan pipeline                         │
└───────────────────────┬─────────────────────────────────────┘
                        │
        ┌───────────────┼────────────────┐
        ▼               ▼                ▼
   Git + GCM       PHP/Composer/npm   Windows SCM/NSSM
```

### 3.1 Model Penjadwalan

Dua opsi, dapat dipilih saat implementasi:

| Opsi | Cara kerja | Kelebihan | Kekurangan |
| --- | --- | --- | --- |
| **A. Task Scheduler** (rekomendasi awal) | Setiap 1-5 menit jalankan `sm-updater run --due` | Tidak perlu daemon; mudah dipantau; berjalan sebagai akun EGOV22 | Butuh task eksternal |
| **B. Windows Service** `GlobalUpdater` | NSSM menjalankan `sm-updater daemon` | Selalu hidup, satu service | Credential DPAPI harus milik akun service; lifecycle lebih kompleks |

**Catatan credential:** DPAPI/GCM terikat pada user yang menyimpan token. Jika service berjalan sebagai `LocalSystem`, token milik `EGOV22` tidak otomatis tersedia. Gunakan salah satu:

- jalankan service sebagai akun `EGOV22` (service logon),
- gunakan deploy key SSH untuk service account,
- simpan token di credential manager akun service.

## 4. Schema Registry (`data/projects.json` versi 2)

Versi 1 tetap dibaca. Saat migrasi, `scanRoots`/`projects` lama dapat diubah menjadi `apps` atau dipertahankan sebagai fallback.

```json
{
  "version": 2,
  "settings": {
    "gitPath": "C:\\laragon\\bin\\git\\cmd\\git.exe",
    "phpPath": "C:\\laragon\\bin\\php\\php.exe",
    "composerPath": "C:\\laragon\\bin\\composer\\composer.bat",
    "npmPath": "C:\\Program Files\\nodejs\\npm.cmd",
    "logDir": "C:\\tools\\ServiceManager\\logs",
    "maxConcurrentUpdates": 1,
    "defaultCheckIntervalMinutes": 5,
    "defaultUpdateWindow": {
      "start": "01:00",
      "end": "05:00"
    }
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
      "autoUpdate": "apply",
      "checkIntervalMinutes": 5,
      "updateWindow": { "start": "01:00", "end": "05:00" },
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
  ]
}
```

### 4.1 Field Wajib Registrasi

| Field | Wajib | Keterangan |
| --- | --- | --- |
| `id` | Ya | Slug unik, huruf kecil/angka/`-` |
| `name` | Ya | Nama tampilan |
| `repoUrl` | Ya | HTTPS/SSH; tidak boleh berisi token/password |
| `localPath` | Ya | Folder project; harus Laravel atau kosong untuk clone |
| `branch` | Ya | Branch yang dipantau |
| `enabled` | Ya | Aktif/nonaktif |
| `autoUpdate` | Ya | `off`, `check`, `apply` |

### 4.2 Field Opsional Penting

- `cloneOnFirstRun` — clone repo jika `localPath` belum ada.
- `checkIntervalMinutes` — interval polling (default 5).
- `updateWindow` — hanya update dalam jam tertentu.
- `composer`/`build` — `auto`, `always`, `never`.
- `migrationPolicy` — saat ini hanya `block` (aman); `allow` tidak disarankan.
- `restartServices` — service Windows yang direstart setelah update.
- `preUpdateCommand`/`postUpdateCommand` — hook PowerShell.
- `notify` — webhook opsional.
- `healthCheck` — cek URL setelah deploy.

## 5. Fitur yang Akan Dibuat

### 5.1 Registrasi Aplikasi

| Fitur | Deskripsi |
| --- | --- |
| Add app (CLI) | `sm-updater app add --id ... --repo ... --path ... --branch ...` |
| Add app (GUI) | Form di tab Updater |
| Interactive add | Prompt bertahap jika argumen tidak lengkap |
| Edit app | Ubah repo/path/branch/policy |
| Remove app | Hapus dari registry (tidak menghapus folder) |
| Enable/disable | Toggle per app |
| List/status | Tabel app + status terakhir |
| Test connection | `git ls-remote` + validasi path/Laravel + branch |
| Clone on first run | Clone repo ke `localPath` bila belum ada |
| Import dari scan | Ubah project hasil scan menjadi registry entry |
| Validasi | ID unik, URL valid, path tidak konflik, branch ada |

### 5.2 Deteksi Perubahan Otomatis

| Fitur | Deskripsi |
| --- | --- |
| Poll remote | `git ls-remote origin refs/heads/<branch>` setiap interval |
| Bandingkan SHA | Bandingkan remote SHA dengan SHA lokal/`origin/<branch>` |
| Fetch selektif | Fetch hanya jika SHA remote berubah |
| Update window | Hanya jalankan pada jam yang diizinkan |
| Jitter/backoff | Hindari semua app polling bersamaan; backoff saat gagal |
| Skip jika dirty | Working tree kotor → status `dirty`, tidak update |
| Skip jika branch salah | Branch aktif ≠ config → status `failed`/blocked |
| Migration guard | Perubahan migration → status `blocked-migration` |
| Lock | Lock global + per app mencegah update bersamaan |
| Retry | Retry terbatas untuk kegagalan jaringan |
| Idempotensi | App yang sudah terbaru tidak diubah |

### 5.3 Pipeline Update

Pipeline yang sudah ada dipertahankan:

1. Validasi repo/path/branch.
2. Cek working tree.
3. Preflight auth.
4. Fetch + fast-forward.
5. Blokir migration.
6. Composer jika perlu.
7. npm ci/install + build jika perlu.
8. `php artisan optimize:clear`.
9. `php artisan queue:restart`.
10. Hook pre/post.
11. Restart service terpilih (opsional).
12. Health check (opsional).
13. Simpan state + log + rollback hint.

Tambahan target:

- `preUpdateCommand` sebelum pull (mis. maintenance mode).
- `restartServices` setelah update.
- `healthCheck` setelah update.
- Status akhir per app disimpan.

### 5.4 State dan Riwayat

File: `data/updater-state.json` (runtime, tidak di-commit).

```json
{
  "version": 1,
  "apps": {
    "kasirin": {
      "status": "up-to-date",
      "lastCheckedAt": "2026-11-16T14:00:00+07:00",
      "lastUpdatedAt": "2026-11-16T13:30:00+07:00",
      "lastCommit": "abc1234",
      "lastError": null,
      "nextCheckAt": "2026-11-16T14:05:00+07:00"
    }
  }
}
```

Fitur:

- Status: `idle`, `checking`, `updating`, `up-to-date`, `blocked-migration`, `dirty`, `failed`.
- Riwayat update (JSONL atau log terpisah) per app.
- Lock file: `data/updater.lock` (PID, timestamp, app).
- Stale lock recovery.
- Retention log (mis. 30 hari / 100 run).

### 5.5 CLI

```powershell
# Registry
sm-updater app add --id kasirin --name Kasirin --repo https://... --path D:\... --branch main --auto apply
sm-updater app list
sm-updater app show kasirin
sm-updater app edit kasirin --branch develop
sm-updater app remove kasirin
sm-updater app enable kasirin
sm-updater app disable kasirin
sm-updater app test kasirin

# Operasi
sm-updater check --app kasirin
sm-updater update --app kasirin
sm-updater run --due                 # dipanggil scheduler
sm-updater run --app kasirin         # paksa cek/update
sm-updater daemon --interval 300     # opsi service
sm-updater status
sm-updater logs kasirin
sm-updater rollback --app kasirin --sha <sha>
```

Kompatibilitas: `list`, `check`, `update`, `--project`, `--all` tetap bekerja.

### 5.6 GUI (Tab Updater)

Tambahan pada `sm-gui`:

- Tab **Services** (sekarang) dan **Updater**.
- Tabel app: name, repo, branch, auto mode, status, last check, last update.
- Tombol: Add, Edit, Remove, Check Now, Update Now, Open Log, Refresh.
- Form registrasi:
  - name/id, repo URL, local path, branch,
  - auto update (off/check/apply), interval, update window,
  - composer/build/queue restart,
  - restart services, pre/post command, webhook.
- Tombol **Test Connection**.
- Badge status + progress saat update.
- Dialog konfirmasi untuk update manual.
- Semua teks Bahasa Inggris dan mendukung dark/light mode.

### 5.7 Notifikasi dan Health Check (Opsional)

- Webhook generic (Discord/Telegram/Slack/custom) untuk `updated`, `failed`, `blocked-migration`.
- Health check HTTP setelah update; gagal → status `failed` + notifikasi.
- Retry/backoff untuk webhook.
- Redaksi token/webhook secret di log.

### 5.8 Integrasi Service Manager

- App updater dapat direstart service tertentu setelah update (mis. queue worker).
- `GlobalUpdater` dapat didaftarkan sebagai service NSSM/GUI Service Manager.
- Status `GlobalUpdater` terlihat di tab Services.
- `queue:restart` tetap default untuk queue Laravel.

## 6. Alur Auto-Update

```
Scheduler (tiap 1-5 menit)
  └─ sm-updater run --due
       ├─ baca registry + state
       ├─ untuk setiap app enabled:
       │    ├─ cek lock (skip jika sedang update)
       │    ├─ cek update window
       │    ├─ git ls-remote (remote SHA)
       │    ├─ jika SHA sama → up-to-date, update state
       │    ├─ fetch + cek migration
       │    ├─ jika migration → blocked-migration + notifikasi
       │    ├─ fast-forward merge
       │    ├─ composer/npm/build/artisan
       │    ├─ queue:restart / restartServices
       │    ├─ health check
       │    └─ simpan state + log + notifikasi
       └─ keluar dengan exit code sesuai hasil
```

## 7. Keamanan

- `repoUrl` tidak boleh mengandung `username:password@`.
- `credentialProfile` hanya referensi host/username; token tetap di GCM/DPAPI.
- Service auto-update harus berjalan sebagai akun yang memiliki akses kredensial.
- `preUpdateCommand`/`postUpdateCommand` hanya dari config yang dikontrol admin.
- `data/` dan `backup/` dibatasi ACL.
- Log di-redaksi; webhook secret tidak ditulis.
- Lock mencegah dua worker.
- Migration tetap manual (tidak auto-run).

## 8. Migrasi Config

- `projects.json` v1 tetap dibaca.
- Perintah `sm-updater migrate-config` mengubah `projects`/`scanRoots` menjadi `apps` tanpa menghapus data.
- Backup otomatis sebelum menulis config baru ke `backup/`.
- Aplikasi yang belum punya `repoUrl` dapat diisi lewat GUI/CLI; update otomatis tidak aktif sampai registrasi lengkap.

## 9. Fase Implementasi

| Fase | Deliverable | Estimasi |
| --- | --- | --- |
| A. Registry & validasi | Schema v2, migrasi v1→v2, `app add/list/edit/remove/test`, clone-on-first-run, test | 3-5 hari |
| B. Engine auto-update | Polling, lock, state, retry, update window, `run --due`, `daemon`, test | 5-7 hari |
| C. GUI Updater | Tab, tabel, form registrasi, test connection, status, check/update now | 5-7 hari |
| D. Integrasi & notifikasi | Restart service, pre/post hook, webhook, health check, rollback | 3-5 hari |
| E. Hardening & rilis | Log retention, ACL, dokumentasi, E2E, release | 2-3 hari |

**Total estimasi:** 18-27 person-days.

## 10. Kriteria Penerimaan

- [ ] App hanya bisa auto-update setelah terdaftar dengan repo URL, path, dan branch.
- [ ] Config v1 tetap terbaca; migrasi v2 tidak kehilangan data.
- [ ] `sm-updater app test` memvalidasi repo, auth, path, branch, dan Laravel.
- [ ] Polling tidak menjalankan update jika SHA remote belum berubah.
- [ ] Lock mencegah update ganda; stale lock dapat dipulihkan.
- [ ] Migration baru selalu diblokir (tidak auto-run).
- [ ] `run --due` dan `daemon` berjalan di Task Scheduler/service.
- [ ] State dan log per app dapat dilihat di CLI dan GUI.
- [ ] GUI dapat add/edit/remove/test/check/update tanpa CLI.
- [ ] Tidak ada token/password di config/log.
- [ ] Semua fitur baru memiliki unit test + update dokumentasi.

## 11. Non-Goals

- Auto-run `php artisan migrate`.
- Menyelesaikan merge conflict otomatis.
- Mengubah branch otomatis.
- Menyimpan token di `projects.json`.
- Menggantikan Git Credential Manager.
- Deploy non-Laravel pada fase awal.

## 12. Pertanyaan Terbuka

1. Auto-update dijalankan lewat **Task Scheduler** atau **Windows Service**?
2. Akun service mana yang akan dipakai (`EGOV22`, akun khusus, atau SSH deploy key)?
3. Apakah update harus dibatasi jam tertentu (mis. malam hari)?
4. Apakah webhook/notifikasi diperlukan? Ke mana?
5. Apakah `cloneOnFirstRun` diperlukan (repo belum ada di server)?
6. Apakah rollback otomatis setelah health check gagal diinginkan?
7. Apakah GUI Updater harus masuk ke aplikasi yang sama atau aplikasi terpisah?

## 13. Rekomendasi Awal

1. Mulai dari **registry + CLI** (Fase A), karena menjadi fondasi semua fitur.
2. Gunakan **Task Scheduler** dengan `sm-updater run --due` sebagai langkah pertama; service `GlobalUpdater` menyusul.
3. Jalankan sebagai akun `EGOV22` yang sudah memiliki kredensial Git.
4. Default `autoUpdate: check` untuk app baru, lalu ubah ke `apply` setelah verifikasi.
5. Pertahankan migration `block` dan backup `backup/` di semua kondisi.
6. Integrasikan GUI setelah engine stabil agar UI tidak menangani logika deploy.

---

## Referensi

- Implementasi saat ini: [RUST-UPDATER.md](RUST-UPDATER.md)
- Inventaris & backlog saat ini: [RUST-UPDATER-PLAN.md](RUST-UPDATER-PLAN.md)
- Service global/queue: [RUST-SERVICES.md](RUST-SERVICES.md)
- Keamanan: [RUST-SECURITY.md](RUST-SECURITY.md)
