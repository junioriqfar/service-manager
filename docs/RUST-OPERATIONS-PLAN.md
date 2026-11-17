# Rencana Fase 3 — Operasional, Doctor, Plan, History

Status: **Sprint 3.1–3.2 selesai; Sprint 3.3–3.5 belum**

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

---

## Sprint 3.4 — Admin tools (opsional, butuh keputusan)

- Credential manager UI (Git PAT store/test/clear).
- Backup/restore settings tanpa rahasia.
- Maintenance mode / pause trigger.
- Firewall helper (read/add/remove rule SSH; admin).
- Test koneksi SSH remote.

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
- [ ] History JSONL dan UI history.
- [ ] Retention manual + tombol Settings.
- [ ] Audit trigger IP/action.
- [ ] Test/clippy/fmt hijau.
- [ ] Docs diperbarui.
