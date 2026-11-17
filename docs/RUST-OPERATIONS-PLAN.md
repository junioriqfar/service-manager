# Rencana Fase 3 — Operasional, Doctor, Plan, History

Status: **rencana / Sprint 3.1 dimulai**

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

---

## Sprint 3.2 — Dry-run Plan

Tujuan: menampilkan rencana update tanpa eksekusi.

Isi plan:

- app, branch, HEAD sebelum;
- incoming commits + changed files;
- migration files yang memblokir;
- toolchain efektif;
- daftar install/build/artisan command (program + args);
- service stop/restart/start;
- health check dan webhook;
- status: `up-to-date`, `dirty`, `blocked-migration`, `ready`, `error`;
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

- [ ] Doctor report lengkap, read-only, redacted.
- [ ] Plan preview tanpa side effect.
- [ ] History JSONL dan UI history.
- [ ] Retention manual + tombol Settings.
- [ ] Audit trigger IP/action.
- [ ] Test/clippy/fmt hijau.
- [ ] Docs diperbarui.
