# Rencana dan Inventaris Fitur `sm-updater`

Dokumen ini merangkum hasil observasi implementasi `sm-updater`, seluruh fitur yang sudah ada, serta fitur yang direncanakan berikut prioritasnya.

- Status implementasi: **Phase 4 selesai** (fitur inti berjalan dan teruji)
- Lokasi: `crates/sm-updater`
- Test: **37 unit test** untuk crate ini; total workspace **180 test**. Config default ada di `data/projects.json`.
- Verifikasi: unit test + E2E lokal dengan repository Git bare (list, check, update fast-forward, migration block)

---

## 1. Tujuan

`sm-updater` adalah CLI deployment Laravel yang aman dan dapat dijalankan tanpa interaksi browser:

- memperbarui kode dari Git tanpa merge konflik,
- menolak update berisiko (working tree kotor, branch salah, migration baru),
- menjalankan langkah build/deploy secara kondisional,
- mencatat log dan rollback hint,
- menyimpan kredensial Git dengan aman.

## 2. Prinsip Desain

1. **Aman secara default**: fast-forward only, tolak working tree kotor, blokir migration otomatis.
2. **Idempoten**: project yang sudah terbaru tidak diubah.
3. **Terukur**: setiap perintah punya timeout dan output dicatat.
4. **Tanpa rahasia di disk/repo**: kredensial lewat GCM/DPAPI; log di-redaksi.
5. **Kompatibel config**: `projects.json` versi 1.
6. **Bisa diotomasi**: exit code dan mode non-interaktif.
7. **Tidak menggantikan Git/migration tool**: fokus pada orkestrasi deploy.

## 3. Fitur yang Sudah Diimplementasikan

### 3.1 CLI dan Mode

| Fitur | Status | Detail |
| --- | --- | --- |
| Menu interaktif | Selesai | `sm-updater` / `sm-updater menu` |
| Daftar project | Selesai | `sm-updater list` |
| Cek update (dry-run) | Selesai | `sm-updater check [--project NAMA]` |
| Update project | Selesai | `sm-updater update [--project NAMA]` |
| Update semua project aktif | Selesai | `sm-updater update` atau `--all` |
| Config path kustom | Selesai | `--config PATH` |
| Bantuan | Selesai | `--help` / `-h` |
| Credential helper | Selesai | `sm-updater credential store` |
| Exit code terstruktur | Selesai | `0` sukses, `1` gagal/config, `2` argumen salah |

### 3.2 Discovery dan Konfigurasi

| Fitur | Status | Detail |
| --- | --- | --- |
| Scan `scanRoots` | Selesai | Satu level direktori, diurutkan berdasarkan nama |
| Deteksi project Laravel | Selesai | `.git` + `artisan` + `composer.json` |
| Override path manual | Selesai | Entry `projects` dengan `path` |
| Project enabled/nonaktif | Selesai | `enabled: false` dilewati pada mode all |
| Branch per project | Selesai | Validasi branch aktif |
| Composer policy | Selesai | `auto`, `always`, `never` |
| Build policy | Selesai | `auto`, `always`, `never` |
| Queue restart policy | Selesai | `queueRestart: true/false` |
| Post-update command | Selesai | Dijalankan via PowerShell |
| Path tool global | Selesai | Git, PHP, Composer, npm, logDir |
| Default log dir | Selesai | Mengikuti parent `projects.json` jika `logDir` kosong |

### 3.3 Alur Git yang Aman

| Fitur | Status | Detail |
| --- | --- | --- |
| Validasi project | Selesai | Cek struktur Laravel |
| Working tree bersih | Selesai | `git status --porcelain`; dirty → skipped |
| Validasi branch | Selesai | Branch aktif harus sama dengan config |
| Preflight remote | Selesai | `git ls-remote --heads origin`, timeout 30 detik |
| Deteksi auth gagal | Selesai | Pesan kredensial yang jelas |
| Fetch | Selesai | `git fetch --prune origin`, timeout 120 detik |
| Cek origin branch | Selesai | `git rev-parse --verify origin/<branch>` |
| Preview commit masuk | Selesai | `git log --oneline HEAD..origin/<branch>` |
| Preview file berubah | Selesai | `git diff --name-only HEAD..origin/<branch>` |
| Fast-forward merge | Selesai | `git merge --ff-only origin/<branch>` |
| Deteksi tidak ada perubahan | Selesai | SHA sebelum = sesudah |
| Diff file setelah merge | Selesai | `git diff --name-only <before> <after>` |
| Rollback hint | Selesai | `git reset --hard <sha-sebelum>` dicatat di log |

### 3.4 Migration Guard

| Fitur | Status | Detail |
| --- | --- | --- |
| Deteksi `database/migrations/*` | Selesai | Termasuk path Windows/backslash |
| `check` tetap melaporkan | Selesai | Status `check` |
| `update` membatalkan pull | Selesai | Status `blocked-migration` |
| Daftar file migration | Selesai | Dicatat di log |
| Auto-run migration | **Tidak** (by design) | Harus manual |

### 3.5 Langkah Build/Deploy Kondisional

| Perubahan | Aksi | Status |
| --- | --- | --- |
| `composer.json`/`composer.lock` | `composer install --no-dev --optimize-autoloader --no-interaction` | Selesai |
| `package-lock.json` | `npm ci` lalu `npm run build` | Selesai |
| `package.json` | `npm install` lalu `npm run build` | Selesai |
| `resources/*`, `vite.config.*`, `tsconfig.json` | `npm run build` | Selesai |
| Cache Laravel ada | `php artisan optimize:clear` | Selesai |
| Kode berubah + `queueRestart` | `php artisan queue:restart` | Selesai |
| `postUpdateCommand` | PowerShell command | Selesai |
| `database/migrations/*` | Blokir update | Selesai |

Catatan: `.bat`/`.cmd` (composer/npm) dijalankan via `cmd.exe /c` dengan path eksplisit.

### 3.6 Status dan Exit Code

| Status | Arti |
| --- | --- |
| `up-to-date` | Tidak ada commit baru / tidak ada perubahan |
| `check` | Dry-run selesai, tidak ada file diubah |
| `updated` | Update dan langkah lanjutan berhasil |
| `dirty` | Working tree tidak bersih; project dilewati |
| `blocked-migration` | Ada migration baru; tidak ada pull |
| `failed` | Error (Git, auth, command, config, dll.) |

### 3.7 Logging

| Fitur | Status | Detail |
| --- | --- | --- |
| Log per run | Selesai | `logDir/project-update-YYYYMMDD-HHmmss.log` |
| Log per project | Selesai | `<project>/storage/logs/project-update-...log` |
| Timestamp lokal | Selesai | `YYYY-MM-DD HH:MM:SS` |
| Log perintah | Selesai | Program + argumen dengan quoting |
| Output perintah | Selesai | Maksimal 60 baris per perintah |
| Commit/file berubah | Selesai | Dicatat |
| Rollback hint | Selesai | SHA sebelum update |
| Redaksi rahasia | Selesai | Token/Bearer/password/userinfo URL |
| Rotasi log updater | **Belum** | Belum ada pruning/retensi |

### 3.8 Kredensial dan Keamanan

| Fitur | Status | Detail |
| --- | --- | --- |
| Token tanpa echo | Selesai | `rpassword` |
| GCM/DPAPI | Selesai | `GCM_CREDENTIAL_STORE=dpapi` |
| Kirim token via stdin | Selesai | `git credential approve` |
| Verifikasi remote | Selesai | `git ls-remote --heads origin` |
| Deteksi path Git/project | Selesai | Validasi sebelum prompt |
| Auto-resolve git | Selesai | `SERVICE_MANAGER_GIT`, Laragon, Program Files, `where git` |
| Timeout proses | Selesai | Preflight 30s, fetch/merge 120s, tool 300s |
| Redaksi error | Selesai | `sm_core::redact_secrets` |
| Locking antar-run | **Belum** | Belum ada lock file |
| Structured JSON output | **Belum** | Hanya teks |

### 3.9 Pengujian

- **37 unit test** crate `sm-updater`:
  - parsing CLI dan credential,
  - scan project/override,
  - runner nyata (exit code/timeout),
  - log dan redaksi,
  - matriks update: up-to-date, check, dirty, branch mismatch, auth failure, migration block, composer failure, post-update command, langkah kondisional.
- **E2E lokal** dengan repository Git bare:
  - `list` mendeteksi project,
  - `check` melaporkan up-to-date/incoming,
  - `update` fast-forward + rollback SHA,
  - migration block tanpa pull.
- **Belum** ada integration test dengan remote GitHub/GitLab nyata atau SSH key.

---

## 4. Fitur yang Direncanakan (Backlog)

Prioritas: **P0** = penting untuk operasi aman, **P1** = peningkatan, **P2** = nice-to-have.

### P0 — Operasi Aman

| Fitur | Manfaat | Rencana |
| --- | --- | --- |
| **Lock file** | Mencegah dua updater berjalan bersamaan pada project yang sama | File `.sm-updater.lock` per project + PID/timestamp; stale lock timeout |
| **Config validation command** | Cek `projects.json` tanpa menjalankan update | `sm-updater validate [--config PATH]` |
| **Structured output** | Memudahkan integrasi Task Scheduler/monitoring | `--json` untuk `list`, `check`, `update` |
| **Log retention** | Mencegah log menumpuk | Simpan N run terakhir / maksimal ukuran |
| **Service restart hook** | Restart queue/cron service setelah deploy | `restartServices`/`postUpdateCommand` yang lebih terstruktur |
| **Confirmation flag** | Cegah update tidak sengaja dari terminal | `--yes` eksplisit untuk mode update non-interaktif |

### P1 — Peningkatan Deploy

| Fitur | Manfaat | Rencana |
| --- | --- | --- |
| **Rollback command** | Rollback cepat ke SHA sebelumnya | `sm-updater rollback --project X --sha <sha>` dengan konfirmasi |
| **Configurable timeouts** | Menyesuaikan project besar/lambat | `timeouts.preflight/fetch/tool` di `projects.json` |
| **Configurable remote** | Remote selain `origin` | `remote: origin` per project |
| **Parallel updates** | Mempercepat banyak project | Batas concurrency, default 1-2 |
| **Notifications** | Info hasil ke admin | Webhook/email/Telegram opsional |
| **Health check** | Pastikan aplikasi hidup setelah deploy | HTTP check/`php artisan about` opsional |
| **Per-project env** | Build/deploy variabel khusus | `env` map per project |
| **Pre-update hook** | Maintenance mode/cache warmup | `preUpdateCommand` |
| **Log detail level** | Debugging | `verbose`/`logLevel` |

### P2 — Lanjutan

| Fitur | Manfaat |
| --- | --- |
| **Built-in scheduler/service mode** | Tidak perlu Task Scheduler eksternal |
| **UI integration** | Tombol update di `sm-gui` dengan progress |
| **Git submodule/LFS handling** | Project yang memakai submodule/LFS |
| **Multi-remote/SSH docs** | Dukungan remote selain HTTPS/GCM |
| **Backup snapshot sebelum deploy** | Rollback file non-Git (mis. storage) |
| **Retry policy** | Retry untuk kegagalan jaringan sementara |
| **Project grouping** | Update grup project (mis. semua project `open-po`) |

---

## 5. Non-Goals

Fitur berikut sengaja **tidak** dilakukan otomatis:

- Menjalankan `php artisan migrate` tanpa persetujuan.
- Menyelesaikan merge conflict otomatis.
- Mengubah branch project secara otomatis.
- Menggantikan Git Credential Manager.
- Menjalankan dua worker queue project berbeda dalam satu proses (tetap per project).
- Auto-update aplikasi Service Manager itu sendiri.

---

## 6. Rencana Implementasi Berikutnya

### Iterasi 1 — Safety & Automation (P0)

1. Lock file per project + stale lock.
2. `validate` command.
3. `--json` output.
4. Log retention.
5. Flag `--yes` untuk update non-interaktif.
6. Service restart hook terstruktur.

### Iterasi 2 — Deploy Control (P1)

1. Rollback command.
2. Configurable timeouts/remote.
3. Parallel update dengan batas concurrency.
4. Notifications.
5. Health check.
6. Pre-update hook dan per-project env.

### Iterasi 3 — Scale (P2)

1. UI integration di `sm-gui`.
2. Scheduler bawaan.
3. Dukungan submodule/LFS.
4. Backup snapshot dan retry policy.

---

## 7. Kriteria Penerimaan Iterasi Berikutnya

- [ ] Lock file mencegah dua run bersamaan; lock stale dapat dipulihkan.
- [ ] `validate` dan `--json` memiliki test dan dokumentasi.
- [ ] Log retention membatasi jumlah/ukuran log.
- [ ] `--yes` wajib untuk update non-interaktif (atau perilaku lama didokumentasikan).
- [ ] Rollback command diuji dengan repository lokal.
- [ ] Timeout/remote config diuji tanpa mengubah default lama.
- [ ] Semua fitur baru memiliki unit test + update `docs/RUST-UPDATER.md`.
- [ ] Tidak ada regresi pada 37 test lama dan E2E lokal.

---

## 8. Pertanyaan Terbuka

1. Apakah updater akan dijalankan dari Task Scheduler (butuh `--yes`/non-interaktif) atau manual?
2. Apakah rollback harus tersedia dari GUI atau CLI saja?
3. Apakah service queue/cron perlu di-restart otomatis setelah deploy?
4. Berapa banyak project yang akan dikelola (menentukan kebutuhan parallel update)?
5. Apakah notifikasi diperlukan (email/Telegram/webhook)?
6. Apakah migration harus tetap 100% manual atau boleh `migrate --pretend` untuk preview?
7. Apakah perlu dukungan remote SSH di samping HTTPS/GCM?

---

## 9. Referensi

- Implementasi: `crates/sm-updater/src/`
- Panduan penggunaan: [RUST-UPDATER.md](RUST-UPDATER.md)
- Config contoh: `projects.example.json`
- Service integration: [RUST-SERVICES.md](RUST-SERVICES.md)
- Security: [RUST-SECURITY.md](RUST-SECURITY.md)
