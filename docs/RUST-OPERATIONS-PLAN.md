# Rencana Fase 3 — Operasional, Doctor, Plan, History

Status: **Sprint 3.1–3.3 dan 3.4a selesai; Sprint 3.4b–3.5 belum**

Lanjutan dari [RUST-SETTINGS-PLAN.md](RUST-SETTINGS-PLAN.md). Fase 3 berfokus pada
observability dan keselamatan operasi, bukan fitur update inti.

## Prinsip

- Read-only untuk Doctor dan Plan; tidak mengubah file/service.
- Tidak ada token/password di report/log.
- Plan tidak menjalankan install/build, tidak stop/restart service, tidak merge.
- Retention tidak menghapus log aktif.
- Setiap fitur: test + docs + commit/push sendiri.

---

## Sprint 3.1 — Doctor / Readiness

Status: **Selesai**.

Tujuan: satu laporan kesiapan server.

Check:

- Administrator / elevation.
- Git, PHP, Composer, npm, Bun, Go, Apache, NSSM:
  path valid + versi.
- Settings: ada/tidak, legacy fallback, valid/tidak.
- Projects: app count, path valid, `webServerService` valid.
- Apache service yang direferensikan: ada di `services.json`/SCM, state.
- Git remote access per app (`ls-remote`), timeout pendek.
- Scheduled Task `ServiceManager-Updater`: installed/state/run level/last result.
- SSH service `sshd` state.
- Storage: `data/`, `logs/`, `backup/` writable; disk free (best effort).
- Pending Apache change.
- PHP reference scan (optional warning).
- Firewall/SSH port (best-effort read-only; action di 3.4).

Output:

- `DoctorReport { generatedAt, overall, checks[] }`.
- Status: `ok`, `warning`, `error`, `skipped`.
- UI card/dialog di Settings: **Run Doctor**, badge overall, grouped checks, **Copy Report**.
- Detail redacted with `sm_core::redact_secrets`.

Tests:

- Aggregasi overall.
- Missing required tool -> error.
- Optional tool missing -> warning.
- Task not installed -> warning.
- Path/config checks with temp dirs.
- UI labels/commands.

Catatan implementasi Sprint 3.1:

- File `crates/sm-gui/src/doctor.rs`: `DoctorStatus` (`ok|warning|error|skipped`),
  `DoctorCheck` (`id, category, label, status, detail, hint`), `DoctorCounts`, dan
  `DoctorReport` (`generatedAt, overall, checks, counts, basePath`).
- Command Tauri `run_doctor` berjalan di `spawn_blocking` dan hanya membaca.
  Satu-satunya write adalah file probe sementara di folder `data/`, `logs/`, dan
  `backup/` yang sudah ada; file langsung dihapus dan folder yang belum ada tidak
  dibuat.
- Environment: status Administrator/elevasi (non-admin warning, bukan error).
- Tools: Git/PHP/Composer/npm wajib; pnpm/yarn/Bun/Go/Apache/NSSM opsional.
  Path efektif dari `settings.json` (fallback legacy `projects.json`), version test
  timeout 8 detik. NSSM hanya diverifikasi keberadaan filenya karena `nssm version`
  membuka GUI. Wajib kosong/gagal = error; opsional kosong/gagal = warning.
- Settings: `settings.json` valid = ok, tidak ada = warning legacy fallback,
  invalid = error. Apache root/config dicek bila path Apache diisi.
- Registry: `projects.json` valid + jumlah app; app enabled dengan `localPath`
  kosong/bukan folder = warning; `webServerService` harus ada di `services.json`
  dan `serviceType=apache`, jika tidak = warning. Duplikat app id = warning
  (config invalid tetap error).
- Services: `services.json` invalid = error; service Apache yang direferensikan
  dicek lewat SCM (tidak terpasang = warning, terpasang = ok + state). Path Apache
  diisi tanpa service Apache = warning.
- Remote access: per app enabled dengan `repoUrl`, `git ls-remote --heads` memakai
  Git efektif, timeout 10 detik, `GIT_TERMINAL_PROMPT=0` dan
  `GCM_INTERACTIVE=never`; sukses = ok + jumlah branch head, gagal = warning.
  Userinfo URL dihapus dari detail.
- Scheduled Task: tidak terpasang, state bukan `Ready`, atau `lastResult != 0`
  = warning.
- SSH: service `sshd` dan port 22 dicek best effort; tidak ada/stopped/port
  tertutup = warning, query error = skipped. Tidak ada perubahan firewall.
- Storage: `data/`, `logs/`, `backup/` ada + writable (probe); disk bebas < 1 GiB
  = warning, gagal baca = skipped. Helper `sm_win::free_space` memakai
  `GetDiskFreeSpaceExW`.
- Pending Apache: `data/apache-pending.json` ada = warning; tidak ada = ok.
- PHP: scan command app untuk path `php.exe` hardcoded yang berbeda dari PHP
  efektif dan bukan `${PHP}`; warning berisi app/field. Tidak ada rewrite.
- Semua `detail`/`hint` di-redaksi dengan `sm_core::redact_secrets` sebelum
  serialisasi, termasuk userinfo URL.
- Batasan: tidak ada auto-fix, tidak ada perubahan firewall, tidak ada verifikasi
  kredensial remote yang interaktif, dan status port SSH hanya untuk IPv4
  `127.0.0.1:22`. Test memakai helper murni/temp dir tanpa SCM, service, task,
  atau network nyata.

---

## Sprint 3.2 — Dry-run Plan

Status: **Selesai**.

Tujuan: menampilkan rencana update tanpa eksekusi.

Isi plan:

- app, branch, HEAD sebelum;
- incoming commits + changed files;
- migration files yang memblokir;
- toolchain efektif;
- daftar install/build/artisan command (program + args);
- service stop/restart/start;
- health check dan webhook;
- status: `up-to-date`, `dirty`, `blocked-migration`, `ready`, `clone-required`, `error`;
- notes/warnings.

Backend:

- `plan_app(app_id)` read-only:
  - preflight Git, fetch, incoming/diff, toolchain resolve, service validation read-only.
  - tidak merge, tidak stop service, tidak menjalankan tool.
- CLI: `sm-updater plan --app ID` (opsional `--all`).
- GUI: action **Preview Plan** per app + dialog.
- Serialisasi command plan.

Tests:

- up-to-date, dirty, migration blocked, ready with toolchain/services.
- No calls to install/build/stop/restart.
- Service validation read-only.
- CLI parse/output smoke test.

Catatan implementasi Sprint 3.2:

- File `crates/sm-updater/src/plan.rs`: `PlanStatus`
  (`up-to-date|dirty|blocked-migration|ready|clone-required|error`),
  `PlanCommit`, `PlanCommand` (group `install|build|artisan`), `PlanService`,
  `PlanServices`, `PlanHealth`, `PlanNotify`, dan `AppPlan` (serde camelCase).
- `plan_app(app, settings, project, runner, services)` tidak pernah panic.
  Kegagalan Git/toolchain/service menjadi `status=error` atau warning, bukan
  crash. `AppPlan.status` juga dipakai untuk `clone-required` dan
  `blocked-migration`.
- Urutan Git read-only: validasi branch/path/git → `git status --porcelain` →
  branch aktif (`rev-parse --abbrev-ref HEAD`) → `rev-parse HEAD` →
  `git ls-remote --heads origin` (timeout 20 detik, prompt dinonaktifkan) →
  `git fetch --prune origin` (timeout 120 detik) →
  `rev-parse --verify origin/<branch>` → `rev-list --count HEAD..origin/<branch>`
  → `git log --format=%H%x09%s` → `git diff --name-only`.
- Working tree kotor: status `dirty`, maksimal 30 entri pertama masuk warnings,
  `git fetch` dan merge tidak dijalankan. Branch mismatch dan Git tidak ada:
  status `error` dengan pesan jelas. Local path tidak ada: `clone-required`
  tanpa clone. Tidak ada commit masuk: `up-to-date`.
- Migration dideteksi dengan `sm_core::update::is_migration_change`; status
  menjadi `blocked-migration`, tetapi file migration dan command toolchain tetap
  ditampilkan agar operator bisa menilai dampaknya.
- `toolchain::resolve_plan` hanya menyusun command (tidak mengeksekusi). Jika
  resolusi gagal (lockfile ambigu, tool tidak ada, folder hilang), command
  dikosongkan dan alasan masuk warnings.
- Service memakai `Lifecycle::validate_readonly` (hanya `exists`) dan
  `is_running` per nama unik. Tidak ada `preflight`, `stop`, `start`, atau
  `restart`. Hasil validasi dan state best-effort masuk `PlanServices`.
- Health check dan webhook hanya informasi; URL di-redaksi dengan
  `sm_core::redact_secrets`, termasuk userinfo/token pada webhook. Tidak ada HTTP
  request.
- **Read-only**: `git fetch --prune` memperbarui remote-tracking refs di `.git`
  dan itu satu-satunya perubahan; tidak ada merge, clone, build, install,
  stop/restart service, tulis log, config, atau state.
- CLI: `sm-updater plan --app ID [--config PATH]`, `sm-updater plan --all`, dan
  alias `--project`. Output memuat status, branch/HEAD, incoming commit, changed
  files, migration, command per group, service, health/webhook, warnings, dan
  catatan fetch read-only. Exit `0` untuk plan yang berhasil dibuat (termasuk
  `ready`/`blocked-migration`/`error`), `1` untuk config/app gagal dimuat, `2`
  untuk argumen tidak valid.
- GUI: command Tauri `plan_updater_app` di `crates/sm-gui/src/updater.rs`
  (spawn_blocking, `SystemRunner` + `WindowsServiceControl`), aksi context menu
  **Preview Plan** sebelum **Run**, dan dialog plan dengan badge status, commit,
  file, migration, command per group, service, health/webhook, warnings, serta
  daftar panjang yang bisa di-scroll.
- Test: 13 test `plan` (up-to-date, ready + command/service, dirty,
  blocked-migration, clone-required, Git hilang, branch mismatch, toolchain
  gagal, service hilang, auth remote gagal, parsing commit, serialisasi status),
  test parse CLI + smoke `collect_plans_at` tanpa SCM/network, test command GUI +
  UI source test.
- Batasan: plan tidak melakukan clone; command yang ditampilkan adalah preview
  untuk perubahan yang terdeteksi saat itu dan bisa berbeda bila remote bergerak;
  health check/webhook tidak dieksekusi; state service best-effort; plan tidak
  menulis history (Sprint 3.3); plan belum memeriksa ancestry/fast-forwardability
  (`merge-base`), jadi `ready` mengasumsikan `git merge --ff-only` akan berhasil;
  CLI membatasi tampilan 30 commit/40 file tetapi payload API tetap lengkap.

---

## Sprint 3.3 — History, Retention, Audit

Status: **Selesai**.

History:

- `data/updater-history.jsonl` append-only:
  `timestamp, appId, status, message, commit, durationMs, clientIp`.
- `load_updater_history(app_id?, limit)`.
- Trigger menambahkan entry per app (termasuk forbidden/up-to-date/failed).
- GUI: dialog **History** per app (50 terakhir).

Retention:

- `prune_logs(log_dir, days)`:
  - `project-update-*.log`, `updater-task-*.log` lebih lama dari N hari;
  - tidak menghapus file yang sedang ditulis (cek nama/current run);
  - prune log project `storage/logs` untuk app terdaftar.
- Settings: tombol **Clean Logs Now** + hasil (file/byte dihapus).
- Jalankan otomatis sekali per `run --all` jika `logRetentionDays > 0`? Rekomendasi:
  manual + tombol; scheduler terpisah.

Audit:

- History trigger sudah memuat IP client.
- Audit Apache tetap di `logs/apache-apply.log`.
- GUI menampilkan history + audit path.

Tests:

- Append/load JSONL, limit/order.
- Prune hanya file lama, keep recent/active.
- Trigger menulis history.
- UI source tests.

Catatan implementasi Sprint 3.3:

- File `crates/sm-updater/src/history.rs`: `HistoryEntry` (`timestamp, appId, status,
  message, commit, durationMs, clientIp`, serde camelCase), `history_path`,
  `append_history`, dan `load_history`.
- Path history adalah sibling `projects.json` (`data/updater-history.jsonl` di layout
  standar). Satu baris = satu JSON object tanpa pretty print; parent folder dibuat
  otomatis. Pesan di-redaksi `sm_core::redact_secrets` lalu dibatasi 4000 karakter
  sebelum serialisasi.
- `load_history` membaca best-effort: file hilang/IO error/malformed line tidak
  menghasilkan error, entry difilter per app secara case-insensitive, hasil
  newest-first, dan `limit` di-clamp ke `1..=500`.
- `run_registry_with` menulis satu history entry untuk setiap app yang diproses:
  `forbidden`, `up-to-date`, `check`, `updated`, `dirty`, `blocked-migration`,
  `degraded`, dan `failed`. `clientIp` berisi IP hasil resolusi trigger
  (`--client-ip`/environment SSH), atau string kosong untuk trigger lokal.
  `commit` adalah HEAD best-effort setelah update (kosong untuk app forbidden karena
  Git tidak dijalankan); `durationMs` adalah durasi update app.
- Kegagalan menulis history hanya dilaporkan ke stderr dan **tidak** mengubah hasil
  run, state, atau exit code. Tidak ada entry history untuk app disabled/tidak
  terpilih, dan tidak ada history yang ditulis oleh plan dry-run.
- Command GUI `load_updater_history(app, limit)` berjalan di `spawn_blocking`,
  memakai `data/projects.json` aktif, dan default limit 50. Dialog **History** di
  context menu tab Updater menampilkan timestamp, badge status, pesan, commit
  pendek, durasi, dan client IP; ada state loading/error/empty dan area scroll.
- File `crates/sm-updater/src/retention.rs`: `PruneResult` (`cutoff, scannedFiles,
  deletedFiles, deletedBytes, skippedActive, errors`, serde camelCase),
  `prune_logs`, dan `prune_project_logs`. Hanya file langsung (bukan folder/symlink)
  dengan nama `project-update-*.log` atau `updater-task-*.log` (case-insensitive)
  yang dipertimbangkan; log project hanya `project-update-*.log` di
  `<localPath>/storage/logs`.
- File yang dimodifikasi dalam `active_grace` (default 5 menit) selalu dihitung
  `skippedActive` dan tidak dihapus. Folder log yang hilang menghasilkan hasil kosong
  tanpa error; error metadata/delete masuk `errors` dan pemrosesan lanjut. Byte yang
  dihapus dihitung dari metadata sebelum delete. `cutoff` adalah ISO timestamp untuk
  ditampilkan.
- Command GUI `prune_logs_now` memakai `settings.json` efektif: `updater.logDir`
  (trim) atau `trigger::default_log_dir(config_path)`, lalu
  `updater.logRetentionDays`. Nilai 0 menghasilkan error
  `Log retention is disabled (0 days).`; command tidak membutuhkan Administrator.
  Retention **manual-only**: tidak ada prune otomatis saat update/run.
- Settings: tombol **Clean Logs Now** di samping **Log retention days**, dialog
  konfirmasi, lalu toast + teks inline berisi jumlah file/byte dihapus, file aktif
  yang dilewati, dan jumlah error. History tidak pernah dihapus oleh aksi ini;
  `logs/apache-apply.log` juga tidak disentuh.
- Test baru: 7 history, 6 retention, 2 trigger (total crate `sm-updater` 152),
  plus 1 GUI history dan 3 GUI retention. Total workspace **413 test**.
- Batasan: history belum dirotasi/di-cap dan `load_history` membaca seluruh file;
  tidak ada index per app. Client IP bergantung pada `SSH_CONNECTION`/`SSH_CLIENT`
  atau `--client-ip`, jadi bisa kosong pada trigger lokal. Proteksi file aktif
  berbasis waktu 5 menit, bukan deteksi proses yang sedang menulis. Retention tidak
  menghapus history, `app.log`, atau audit Apache, dan tidak berjalan otomatis lewat
  scheduler. Helper `prune_logs` dengan `days = 0` akan memakai cutoff sekarang;
  nilai 0 hanya ditolak secara eksplisit pada command GUI.

---

## Sprint 3.4a — Maintenance mode + Settings export/load

Status: **Selesai**.

Tujuan: menjeda update dan trigger SSH selama deploy window, serta memindahkan
settings efektif antar mesin tanpa membawa rahasia. Git credential UI, firewall
UI, dan parallel updates tetap di 3.4b/3.5.

Maintenance:

- `settings.json.maintenance` berisi `{ "enabled": bool, "message": string }`
  (serde camelCase, default `false` dan string kosong). `message` di-trim,
  maksimal 500 karakter, dan menolak karakter kontrol.
- `run_registry_with` menolak **non-check** run sebelum app/Git disentuh jika
  maintenance aktif. Pesan error menjelaskan bahwa update dan trigger SSH dijeda,
  menyertakan `message` kustom bila tidak kosong, dan mengarahkan user ke Settings.
- `run --check` dan dry-run plan tetap berjalan read-only; history/state tidak
  ditulis untuk run yang diblokir. Maintenance tidak mengubah allowlist IP,
  elevation, Scheduled Task, atau autentikasi.
- Doctor menambahkan check `maintenance.status` (kategori `maintenance`):
  enabled = warning dengan hint "Updates and SSH trigger are paused. Disable
  maintenance mode in Settings when ready."; disabled = ok.
- GUI Settings memiliki kartu **Maintenance Mode** dekat Trigger/Privileged
  Execution: switch **Pause updates and SSH trigger**, input **Maintenance
  message** (placeholder "Deploy window / maintenance reason"), penjelasan bahwa
  check dan dry-run plan tetap bekerja, serta tooltip. Nilai disimpan lewat
  **Save Settings**.
- Tab Updater memuat `load_maintenance_status` saat mount/refresh dan menampilkan
  banner peringatan di atas tabel berisi message. Tidak ada tombol disable di
  banner; maintenance hanya dinonaktifkan dari Settings.

Export/load settings:

- Helper murni `export_settings_at(settings_file, projects_file, destination)`
  memuat settings efektif, memvalidasi, dan menulis pretty JSON secara atomik
  (temp + rename) ke destination. Hanya settings yang diekspor: tidak ada
  `projects.json`, `services.json`, log, history, password, atau token.
- `load_settings_from_file_at(path)` membaca, menormalisasi, dan memvalidasi file
  tanpa menulis apa pun.
- Path wajib berekstensi `.json` (case-insensitive) untuk export dan load.
- Command Tauri `export_settings_file` memakai `rfd::FileDialog` dengan default
  `settings-export-<tanggal>.json`; `load_settings_file` membuka file `.json`.
- GUI Settings memiliki tombol **Export Settings…** dan **Load Settings File…**.
  Export menampilkan toast berisi path. Load hanya mengganti draft (tidak
  menyimpan, tidak mengubah service/Apache), menandai **Unsaved changes**, dan
  menampilkan toast "Settings loaded. Review and Save Settings.".
- `.json` yang tidak valid, settings yang gagal validasi, dan message dengan
  karakter kontrol ditolak; `settings.json`/`projects.json` aktif tidak berubah
  selama export/load.

Batasan:

- Maintenance mode bukan kontrol autentikasi dan tidak menggantikan allowlist IP,
  elevation, atau wrapper task; ia hanya menolak run non-check lebih awal.
- Export adalah salinan settings efektif, bukan full backup. Restore penuh tetap
  memakai `projects.json`/`services.json`/`backup` yang ada.
- `variables` admin ikut terekspor; nilai dengan nama sensitif ditolak, tetapi
  operator tetap tidak boleh menaruh rahasia dengan nama tidak sensitif.
- Load tidak menerapkan perubahan otomatis; user harus meninjau draft dan klik
  **Save Settings**. Pending Apache dan dialog PHP references baru dihitung saat
  Save.
- Git credential UI, firewall helper, backup/restore penuh, dan parallel updates
  belum termasuk 3.4a.

## Sprint 3.4b — Admin tools (sisa, opsional)

- Credential manager UI (Git PAT store/test/clear) — 3.4b.
- Backup/restore settings tanpa rahasia — **selesai 3.4a** (export/load efektif).
- Maintenance mode / pause trigger — **selesai 3.4a**.
- Firewall helper (read/add/remove rule SSH; admin) — 3.4b.
- Test koneksi SSH remote — 3.4b.

## Sprint 3.5 — Parallel updates (opsional)

- `maxConcurrentUpdates` nyata; per-app lock.
- Batas restart service bersamaan.
- Progress per app.
- Rollback terisolasi.
- Ditunda sampai 3.1–3.3 stabil.

---

## Acceptance Fase 3 inti

- [x] Doctor report lengkap, read-only, redacted.
- [x] Plan preview tanpa side effect.
- [x] History JSONL dan UI history.
- [x] Retention manual + tombol Settings.
- [x] Maintenance mode + banner Updater + Doctor warning.
- [x] Export/load settings tanpa rahasia.
- [x] Audit trigger IP/action.
- [x] Test/clippy/fmt hijau.
- [x] Docs diperbarui.
