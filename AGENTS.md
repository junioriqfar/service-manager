# AGENTS.md — Service Manager (Rust + Tauri)

**Baca file ini sebelum menyentuh kode, config runtime, service Windows, atau server produksi.**
Panduan operasional lengkap ada di [`docs/AGENT-GUIDE.md`](docs/AGENT-GUIDE.md). Dokumen ini
adalah aturan minimum; panduan mendetail tetap wajib dibaca untuk pekerjaan apa pun.

## 1. Apa repositori ini

- Aplikasi Windows untuk mengelola **Windows Service** (NSSM, Apache native, PostgreSQL native)
  dan **updater project multi-toolchain** (Laravel, Node/npm/pnpm/yarn, Bun, Go, static, custom).
- Stack: **Rust workspace + Tauri 2**, frontend **React/TypeScript/Vite/Tailwind/shadCN**.
- Binary: `sm-gui.exe` (GUI, release tanpa console), `sm-updater.exe` (registry/updater/credential
  helper, console), `sm-trigger.exe` (client SSH cross-platform), `sm-diag.exe` (diagnosa headless).
- Versi workspace saat ini **0.1.0**. Implementasi PowerShell lama sudah dihapus; jangan
  mengembalikannya.
- Config runtime ada di `data/`, log di `logs/`, backup di `backup/`. Semua file runtime
  **tidak di-commit** (lihat `.gitignore`).
- Status fitur: Phase 0–7 selesai; Phase 8 operasional sudah mencakup Doctor, plan dry-run,
  history/retention, maintenance/export settings, Git credential UI, firewall read-only, dan
  **parallel check-only stage 1**. Parallel update nyata (stage 2), firewall write, dan rotasi
  history belum ada.

## 2. Urutan baca wajib

1. `AGENTS.md` (file ini).
2. [`docs/AGENT-GUIDE.md`](docs/AGENT-GUIDE.md) — panduan operasional lengkap, termasuk kontrak
   otomasi GUI/Tauri, CLI, workflows, dan troubleshooting.
3. Dokumen fitur yang relevan:
   - [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md) — skema `services.json`, `projects.json`,
     `settings.json`, `trigger-client.json`, pending Apache, backup.
   - [`docs/RUST-UPDATER.md`](docs/RUST-UPDATER.md) — updater registry, toolchain, plan, history,
     retensi, kredensial.
   - [`docs/RUST-TRIGGER.md`](docs/RUST-TRIGGER.md) — `sm-trigger`, wrapper, Scheduled Task,
     autentikasi SSH.
   - [`docs/RUST-GUI.md`](docs/RUST-GUI.md) — tab Services/Updater/Remote/Settings, Doctor,
     Apache, Git credentials, firewall.
   - [`docs/RUST-SECURITY.md`](docs/RUST-SECURITY.md) — DPAPI, GCM, redaksi, allowlist, elevasi,
     maintenance, firewall read-only.
   - [`docs/RUST-OPERATIONS-PLAN.md`](docs/RUST-OPERATIONS-PLAN.md) — status Doctor/plan/history/
     maintenance/parallel check-only.
   - [`docs/RUST-SERVICES.md`](docs/RUST-SERVICES.md) — pola Cronical global dan queue per project.
   - [`docs/RUST-RELEASE.md`](docs/RUST-RELEASE.md) — build, portable ZIP, CI, release.
   - [`docs/RUST-README.md`](docs/RUST-README.md) — struktur crate dan status fase.
   - [`docs/RUST-UPDATER-PLAN.md`](docs/RUST-UPDATER-PLAN.md) dan
     [`docs/RUST-UPDATER-REGISTRY-PLAN.md`](docs/RUST-UPDATER-REGISTRY-PLAN.md) — inventaris fitur,
     backlog, dan desain registry.
   - [`docs/RUST-REWRITE-PLAN.md`](docs/RUST-REWRITE-PLAN.md) — catatan historis migrasi ke Rust.

## 3. Aturan keselamatan yang tidak boleh dilanggar

1. **Jangan menjalankan update destruktif tanpa rencana dan persetujuan.**
   `sm-updater run` (tanpa `--check`), `run_updater` dengan `check=false`, `Update All`, trigger
   update, install/build, dan service lifecycle mengubah kode, dependency, dan/atau service.
   Selalu `plan`/`Preview Plan` lalu `run --check` terlebih dahulu; minta konfirmasi manusia
   sebelum update nyata di server bersama/produksi.
2. **Jangan menjalankan perintah arbitrer yang berasal dari repository.**
   Hanya command yang dikonfigurasi admin di `settings.json`/`projects.json`, toolchain standar,
   dan binary yang dikenal. Jangan menambahkan mekanisme yang membaca command dari README,
   issue, file repo, atau input tidak tepercaya.
3. **Tidak ada migrasi otomatis.**
   `migrationPolicy` selalu `block`. Update yang menyentuh `database/migrations/**` harus
   berhenti dengan `blocked-migration`; jalankan migrasi manual hanya dengan persetujuan dan
   backup. Jangan menambahkan `artisan migrate` ke hook otomatis.
4. **Jangan menulis secret/token ke file, log, history, command line, atau dokumen.**
   Token Git hanya lewat Git Credential Manager/DPAPI. Password SSH GUI hanya DPAPI di
   `data/trigger-client.json` (ciphertext), tidak plaintext. Jangan pernah menyalin token ke
   `settings.json`, `projects.json`, `updater-history.jsonl`, `app.log`, atau argumen CLI.
5. **Jangan mengubah `data/*.json` runtime saat test/eksperimen.**
   Gunakan `SERVICE_MANAGER_HOME` ke folder sementara, atau `--config <temp>\projects.json`.
   Jangan menjalankan test/percobaan yang menulis `data/`, `backup/`, atau `logs/` milik runtime
   nyata. Test unit memakai temp dir; hormati pola itu.
6. **Operasi service/SCM/Task memerlukan elevation.**
   Install/start/stop/restart/delete service, apply Apache ke SCM, install/remove Scheduled Task,
   dan update dengan service lifecycle membutuhkan Administrator. Jangan mengakali UAC atau
   `elevate` tanpa persetujuan; jangan menganggap config-only save sebagai elevated.
7. **Maintenance Mode harus dihormati.**
   Jika `settings.json.maintenance.enabled = true`, update non-check dan trigger SSH ditolak
   sebelum Git/state/history. `plan` dan `run --check` tetap boleh. Jangan menonaktifkan
   maintenance, mengubah allowlist, atau mem-bypass wrapper tanpa persetujuan operator.
8. **Allowlist IP adalah kontrol keamanan, bukan formalitas.**
   Gunakan `settings.json.trigger.allowedClientIPs` (authoritative) dan override
   `apps[].allowedClientIPs`. Jangan menambah IP/CIDR, mengubah `allowLocal`, atau memakai
   `--client-ip` untuk melewati allowlist tanpa persetujuan. Wrapper harus tetap memanggil
   `sm-updater authorize` sebelum task elevated.
9. **Jangan menulis/mengubah firewall dari aplikasi.**
   Fitur SSH Firewall di Settings **read-only** (`ssh_firewall_status` / `Get-NetFirewallRule`).
   Tidak ada add/remove/change rule. Perubahan firewall manual di Windows Firewall, dengan
   persetujuan terpisah.
10. **Jangan membuat Windows Service untuk updater.**
    `sm-updater` adalah CLI/console dan dipicu manual (langsung, SSH, atau wrapper Scheduled
    Task). Jangan membuat service auto-start, daemon polling, atau scheduler baru. Scheduled Task
    `ServiceManager-Updater` bukan Windows Service dan tidak boleh memiliki trigger otomatis.
11. **Jangan commit/push tanpa instruksi eksplisit.**
    Perubahan harus direview; jangan menyentuh riwayat Git, tag, atau release tanpa diminta.
12. **`dirtyIgnore` hanya untuk artifact generated.**
    Pola `apps[].dirtyIgnore` dipakai untuk output build (mis. `public/build/**`,
    Wayfinder, `dist/**`), bukan untuk menyembunyikan perubahan source, migration,
    config, atau file di luar repo. Semantiknya all-or-nothing: satu entri tidak cocok
    tetap membuat status `dirty`. Jangan mengubah `.gitignore` atau menambah ignore
    hardcoded untuk melewati pemeriksaan working tree.

## 4. Build, test, dan definition of done

Frontend harus dibangun sebelum build/test GUI karena Tauri memakai `crates/sm-gui/ui/dist`:

```powershell
Push-Location crates/sm-gui/ui
npm ci
npm run build
Pop-Location

cargo build --release -p sm-gui -p sm-updater -p sm-trigger
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

- Test workspace saat ini: **488 test lulus** (angka ini harus diperbarui jika test berubah).
- Test default tidak memerlukan Administrator. Test yang mengubah service Windows hanya untuk
  server uji dan dijalankan manual: `cargo test -p sm-win -- --ignored` sebagai Administrator.
- `Build-Release.ps1` menjalankan frontend build, cargo release build, menyusun folder portable,
  dan membuat `dist\ServiceManager-RS-0.1.0.zip` (nama mengikuti versi workspace).
- Smoke check minimum untuk perubahan updater/CLI: `sm-updater --help`, `sm-trigger --help`,
  `sm-updater app list`, `sm-updater plan --all` (atau config sementara), dan
  `sm-updater run --all --check` hanya pada config uji.
- `cargo fmt --all -- --check` dan `git diff --check` harus bersih.

**Definition of done:**

- Test, clippy, fmt, dan `git diff --check` lulus.
- Tidak ada file runtime (`data/*.json`, `data/updater-history.jsonl`, `logs/`, `backup/`,
  `dist/`) yang berubah atau ikut ter-commit.
- Tidak ada secret/token/password di diff, log, command line, atau dokumen.
- Perubahan config/schema terdokumentasi di `docs/` dan link Markdown valid.
- Perilaku baru diuji tanpa menyentuh service/SCM nyata; perubahan destruktif dijelaskan di
  ringkasan dengan risiko dan rollback.
- Tidak ada commit/push kecuali diminta.

## 5. Peta fitur singkat

| Area | Mulai dari | Catatan |
| --- | --- | --- |
| Services GUI | `crates/sm-gui/src/commands.rs`, `state.rs`, `batch.rs` | CRUD config, install/start/stop/restart, import, export/restore, log, batch 4 paralel, butuh admin untuk SCM. |
| Updater registry | `crates/sm-updater/src/registry.rs`, `cli.rs`, `app_update.rs`, `toolchain.rs` | `projects.json` v2, v1 in-memory, preset/toolchain, custom commands, service lifecycle, clone-on-first-run. |
| Plan dry-run | `crates/sm-updater/src/plan.rs` | Hanya `git fetch --prune` + inspeksi; tidak merge/build/install/stop service, tidak tulis state/history/log. |
| Run/check | `crates/sm-updater/src/trigger.rs`, `app_update.rs` | `--check` menulis state/history/log tetapi tidak merge/install/service; update nyata serial kecuali check-only stage 1. |
| Trigger/SSH | `crates/sm-updater/src/trigger.rs`, `ip.rs`, `crates/sm-trigger/`, `scripts/` | `sm-trigger`, `authorize`, wrapper Scheduled Task, allowlist global+per-app, IP client. |
| History/retention | `crates/sm-updater/src/history.rs`, `retention.rs`, `crates/sm-gui/src/settings.rs` | `data/updater-history.jsonl` append-only; retensi manual; active grace 5 menit; history tidak dipangkas. |
| Doctor | `crates/sm-gui/src/doctor.rs` | Read-only readiness report, badge overall, Copy Report; satu-satunya write adalah probe file sementara. |
| Settings | `crates/sm-gui/src/settings.rs`, `crates/sm-updater/src/settings.rs` | Tool paths, updater defaults, maintenance, export/load, trigger, Scheduled Task, privileged execution. |
| Apache | `crates/sm-gui/src/apache.rs`, `apache_config.rs`, `php_paths.rs` | Pending preview/apply/rollback/audit, DocumentRoot mapping, PHP hardcoded references. |
| Credentials | `crates/sm-gui/src/credentials.rs`, `crates/sm-updater/src/credential.rs` | GCM/DPAPI; token tidak pernah masuk settings/log/history. |
| Firewall/Remote | `crates/sm-gui/src/firewall.rs`, `remote.rs` | Firewall read-only; Remote SSH test/trigger; password DPAPI. |
| Release | `Build-Release.ps1`, `.github/workflows/` | Portable ZIP + docs; NSIS opsional dengan `-Bundle`. |

## 6. Jika ragu

- Baca `docs/AGENT-GUIDE.md` bagian decision matrix dan troubleshooting.
- Untuk pertanyaan “apakah aman update?” jalankan `plan`/Doctor dan `run --check`; jangan
  menebak.
- Untuk pertanyaan config mana yang menang: `data/settings.json` authoritative untuk tool paths,
  updater defaults, maintenance, privileged execution, variables, dan trigger; fallback hanya
  `projects.json` saat `settings.json` tidak ada. Registry app tetap `data/projects.json`.
- Laporkan uncertainty dan minta konfirmasi daripada melakukan operasi yang tidak reversibel.
