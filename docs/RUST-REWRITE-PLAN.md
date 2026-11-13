# Rencana Migrasi Service Manager ke Rust

Dokumen perencanaan lengkap untuk menulis ulang **Service Manager** (PowerShell + WinForms) menjadi aplikasi **Rust** yang setara fitur, aman, dan dapat dipelihara.

- Status: **Draft / perencanaan**
- Basis kode acuan: `ServiceManager.ps1` (3.225 baris), `ProjectUpdate.ps1` (574 baris), `Store-GitCredential.ps1`
- Target platform: Windows 10/11 dan Windows Server (x64)
- Prinsip: **kompatibel config**, **perilaku setara**, **tanpa kredensial di disk/repo**

---

## 1. Ringkasan Eksekutif

Aplikasi saat ini adalah GUI WinForms yang membungkus tiga hal:

1. Windows Service Control Manager (SCM) melalui `nssm.exe`, `httpd.exe -k`, dan `pg_ctl.exe`.
2. Pembacaan/penulisan konfigurasi JSON (`services.json`, `projects.json`).
3. Updater project Laravel berbasis Git (`ProjectUpdate.ps1`).

Seluruh inti fungsional tersebut dapat dipindahkan ke Rust. Yang berubah secara signifikan hanyalah lapisan UI dan cara memanggil API Windows. Risiko utama bukan pada fitur, melainkan pada **paritas perilaku** (status `SERVICE_START_PENDING`, elevasi UAC, penanganan config rusak, dan keamanan kredensial). Rencana ini memecah pekerjaan menjadi fase yang dapat diuji dan dirilis bertahap, dengan aplikasi PowerShell tetap berjalan berdampingan sampai Rust dinyatakan stabil.

**Rekomendasi stack:** Tauri 2 (UI) + crate `windows` (SCM/DPAPI) + `serde` (config) + `tokio` (runtime Tauri) + `tracing` (log). NSSM/Apache/PostgreSQL tetap dependency eksternal dan dipanggil sebagai proses, bukan di-reimplementasi.

**Estimasi total:** 30-45 person-days untuk paritas fitur penuh + pengujian, dibagi 6 fase.

---

## 2. Tujuan dan Non-Tujuan

### 2.1 Tujuan

- Paritas fitur dengan `ServiceManager.ps1`: kelola service NSSM, Apache native, PostgreSQL native.
- Paritas fitur dengan `ProjectUpdate.ps1`: cek/update project Laravel dengan pengamanan migration dan kredensial.
- Kompatibel baca/tulis dengan `services.json` dan `projects.json` versi 1.
- Aplikasi single-binary/installer yang tidak membutuhkan PowerShell.
- Keamanan lebih baik: tidak ada string shell injection, kredensial hanya di Windows Credential Manager/DPAPI.
- Kode modular, typed, dan dapat diuji (unit + integration test).
- Log dan pesan error tetap ramah pengguna (Bahasa Indonesia).

### 2.2 Non-Tujuan

- Tidak membuat ulang NSSM, Apache, atau PostgreSQL.
- Tidak mengganti Git Credential Manager; helper tetap memakai GCM/DPAPI.
- Tidak menambah fitur baru di luar paritas pada fase awal (misalnya remote agent, REST API, database).
- Tidak mendukung Linux/macOS pada versi pertama.
- Tidak melakukan auto-update aplikasi pada fase awal.

---

## 3. Analisis Aplikasi Saat Ini

| Komponen | Isi | Catatan migrasi |
| --- | --- | --- |
| `ServiceManager.ps1` | GUI WinForms, 3.225 baris, ±80 fungsi | Dipecah menjadi crate `sm-core`, `sm-win`, `sm-config`, `sm-gui` |
| `ProjectUpdate.ps1` | CLI/menu updater Laravel, 574 baris | Menjadi binary `sm-updater` (crate `sm-updater`) |
| `Store-GitCredential.ps1` | Menyimpan PAT ke GCM dengan DPAPI | Menjadi subcommand `sm credential store` atau tetap script opsional |
| `services.json` | Config service runtime | Tetap format JSON, tidak di-commit |
| `projects.json` | Config project updater | Tetap format JSON, tidak di-commit |
| `backup/` | Backup config otomatis (maks. 20) | Dipertahankan |
| `logs/` | `app.log` + log updater | Dipertahankan, ditambah rotasi ukuran |

### 3.1 Fitur Service Manager yang Wajib Diport

- Tabel service: status, nama, display name, jenis, executable, startup, restart.
- CRUD config + install/uninstall service.
- Start/Stop/Restart satu service dan semua service.
- Urutan dependensi (topological sort; stop = kebalikannya).
- Tipe service: `nssm`, `apache`, `postgresql`.
- Startup: `auto`, `delayed`, `manual`.
- Restart on crash + delay (NSSM).
- Import service terpasang via WMI/CIM.
- Log viewer (tail, auto-refresh 2 detik, buka file).
- Backup otomatis + export/restore config.
- Diagnosa (`-Diagnose`).
- Elevasi Administrator otomatis.
- Konfirmasi sebelum aksi destruktif (hapus, stop semua, dll).

### 3.2 Fitur Project Update yang Wajib Diport

- Scan `scanRoots` untuk project Laravel (`.git`, `artisan`, `composer.json`).
- Mode `Menu`, `List`, `Check`, `Auto`.
- Filter `-Project`, `-All`, `-ConfigPath`, `-Menu`.
- Preflight `git ls-remote`, `git fetch --prune`.
- Deteksi working tree kotor dan branch tidak sesuai.
- Blokir update otomatis bila ada perubahan `database/migrations/*`.
- `git merge --ff-only`.
- Composer/npm/build/optimize:clear/queue:restart kondisional.
- `postUpdateCommand`.
- Log ke `logs/` dan `storage/logs` project.
- Exit code non-zero bila ada kegagalan.

---

## 4. Keputusan Teknologi

| Area | Pilihan | Alternatif | Alasan |
| --- | --- | --- | --- |
| GUI | **Tauri 2** (Rust + web UI) | `egui`, `iced`, `Slint` | Tabel besar, dialog, dan log viewer lebih cepat dibangun dengan HTML/CSS; bundel installer MSI/NSIS; footprint kecil |
| Windows API | **crate `windows`** | `winapi`, `windows-sys` | Binding resmi Microsoft, typed, mendukung SCM, token, DPAPI, ShellExecute |
| Config | **`serde` + `serde_json`** | `toml`, `ron` | Kompatibel langsung dengan `services.json`/`projects.json` |
| Async | **`tokio`** (bawaan Tauri) | `async-std` | Operasi service tetap sinkron di `spawn_blocking`; hindari blocking UI thread |
| Log | **`tracing` + `tracing-subscriber`** | `log` | Structured logging, filter level, file + stderr |
| Error | **`thiserror`** (domain) + `anyhow` (boundary) | - | Pesan error typed per modul, tetap ramah pengguna |
| Git | **proses `git.exe`** | `git2` (libgit2) | Mempertahankan perilaku Credential Manager/DPAPI dan opsi config yang sudah ada |
| HTTP/Tauri | `tauri` + `@tauri-apps/api` | - | IPC aman, capability-based permission |
| Test | `cargo test`, `tauri-driver`/WebDriver opsional | - | Unit test cepat; integration test Windows-only |

### 4.1 Catatan Pemilihan GUI

- **Tauri**: UI deklaratif, mudah meniru grid dan dialog; butuh WebView2 (sudah tersedia di Windows 10/11 modern).
- **egui/iced/Slint**: binary native murni tanpa WebView, tetapi membangun tabel + dialog + log viewer lebih banyak kerja dan aksesibilitas terbatas.
- Keputusan: **Tauri**. Bila lingkungan target tidak boleh memakai WebView2, siapkan rencana cadangan `Slint` untuk fase lanjutan (bukan scope awal).

---

## 5. Arsitektur Target

### 5.1 Struktur Workspace

```
service-manager-rs/
├─ Cargo.toml                  # workspace
├─ crates/
│  ├─ sm-core/                 # domain model, aturan bisnis, dependency sort
│  │  ├─ src/
│  │  │  ├─ lib.rs
│  │  │  ├─ model.rs           # ServiceConfig, ServiceType, StartupMode, ProjectConfig
│  │  │  ├─ ordering.rs        # topological sort start/stop
│  │  │  ├─ validation.rs      # validasi nama/path/tipe
│  │  │  └─ error.rs
│  ├─ sm-config/               # baca/tulis JSON, backup, contoh config
│  │  └─ src/{lib,services,projects,backup}.rs
│  ├─ sm-win/                  # platform Windows
│  │  └─ src/
│  │     ├─ scm.rs             # OpenSCManager, QueryServiceStatusEx, Start/Stop
│  │     ├─ nssm.rs            # wrapper nssm.exe
│  │     ├─ native.rs          # Apache httpd -k, PostgreSQL pg_ctl
│  │     ├─ discovery.rs       # EnumServicesStatusEx / WMI
│  │     ├─ delayed.rs         # ChangeServiceConfig2
│  │     ├─ elevation.rs       # cek token admin, ShellExecuteW runas
│  │     └─ dpapi.rs           # CryptProtectData/CryptUnprotectData
│  ├─ sm-updater/              # logika Project Update + binary CLI
│  │  └─ src/{lib,git,steps,log,cli}.rs
│  └─ sm-gui/                  # aplikasi Tauri
│     ├─ src/{main.rs,commands.rs,state.rs}
│     └─ ui/                   # HTML/CSS/TS (Svelte/React/Vanilla)
├─ tests/                      # integration test Windows
├─ docs/
└─ .github/workflows/build.yml
```

### 5.2 Lapisan

```
┌──────────────────────────────────────────────┐
│ sm-gui (Tauri commands / UI)                 │
├──────────────────────────────────────────────┤
│ sm-updater CLI        │ sm-core (domain)     │
├───────────────────────┴──────────────────────┤
│ sm-win (SCM, NSSM, native, DPAPI, discovery) │
├──────────────────────────────────────────────┤
│ sm-config (JSON, backup)                     │
└──────────────────────────────────────────────┘
```

Aturan dependensi: `sm-core` tidak boleh bergantung pada `sm-win`; `sm-gui` dan `sm-updater` memanggil API domain, bukan Win32 langsung.

### 5.3 State Management GUI

- Satu `AppState` di Rust: `Vec<ServiceConfig>` + status cache + selected service.
- UI memanggil command Tauri (`list_services`, `start_service`, `save_service`, dst.).
- Operasi lambat (start/stop, import, backup) dijalankan di `spawn_blocking`.
- Event push dari Rust ke UI untuk progress/log live (`service-status-changed`, `log-appended`, `operation-progress`).

---

## 6. Model Data

### 6.1 ServiceConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceConfig {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub service_type: ServiceType,
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub arguments: String,
    #[serde(default)]
    pub working_directory: String,
    #[serde(default)]
    pub apache_root: String,
    #[serde(default)]
    pub apache_config: String,
    #[serde(default)]
    pub pg_data: String,
    #[serde(default)]
    pub pg_account: String,
    #[serde(default)]
    pub stdout_log: String,
    #[serde(default)]
    pub stderr_log: String,
    #[serde(default)]
    pub startup: StartupMode,
    #[serde(default = "default_true")]
    pub restart_on_crash: bool,
    #[serde(default = "default_delay")]
    pub restart_delay_sec: u32,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub added_at: String,
    #[serde(default)]
    pub updated_at: String,
}
```

- Enum: `ServiceType { Nssm, Apache, PostgreSql }`, `StartupMode { Auto, Delayed, Manual }`.
- Nilai tidak dikenal saat deserialisasi: fallback ke default + catat warning (sama seperti PowerShell).
- Field tak dikenal di JSON: diabaikan (default serde), tetapi nilai dikenal dipertahankan saat round-trip.
- Tanggal disimpan sebagai string ISO 8601 agar identik dengan config lama.

### 6.2 ProjectConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverride {
    #[serde(default)]
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default)]
    pub composer: ToggleMode,   // auto | always | never
    #[serde(default)]
    pub build: ToggleMode,
    #[serde(default = "default_true")]
    pub queue_restart: bool,
    #[serde(default)]
    pub post_update_command: String,
}
```

### 6.3 Kompatibilitas

- `version: 1` dipertahankan.
- Tulis ulang dengan pretty JSON dan urutan field yang stabil.
- Uji golden file: baca config lama -> serialisasi -> bandingkan semantik (bukan byte).
- Backup sebelum tulis, maksimal 20 file, pola nama sama: `services-YYYYMMDD-HHmmss-fff.json`.

---

## 7. Pemetaan PowerShell ke Rust

| Fungsi PowerShell | Modul Rust | Catatan |
| --- | --- | --- |
| `Read-Config`, `Save-Config`, `Backup-Config` | `sm-config::services` | Validasi + backup |
| `ConvertTo-ServiceConfig` | `sm-core::model` + `validation` | Normalisasi default |
| `Get-ManagedServicesInStartOrder` | `sm-core::ordering` | Topological sort |
| `Invoke-Nssm`, `Get/Set-NssmValue` | `sm-win::nssm` | Argumen array, bukan string shell |
| `Apply-NativeServiceConfig`, `Get-NativeInstallPlan` | `sm-win::native` | `httpd -k`, `pg_ctl register` |
| `Apply-ServiceConfig`, `Set-ServiceDelayedStart` | `sm-win::scm`, `sm-win::delayed` | `ChangeServiceConfig2` |
| `Test-ServiceExists`, `Get-ServiceStatus`, `Wait-ServiceState` | `sm-win::scm` | Polling dengan timeout |
| `Start/Stop/Restart-ManagedService` | `sm-win::scm` + `nssm`/`native` | Operasi per tipe |
| `Install-ManagedService`, `Remove-ManagedServiceFromWindows` | `sm-win` | Idempoten, verifikasi pasca-aksi |
| `Get-ManagedServiceCandidates`, `ConvertFrom-*Service` | `sm-win::discovery` | `EnumServicesStatusEx` / WMI |
| `Refresh-Grid`, `Update-StatusCells` | `sm-gui::state` | Timer 3 detik |
| `Read-LogTail`, `Update-LogView`, `Select-LogPath` | `sm-win`/`sm-gui` | Tail N baris, auto-refresh 2 detik |
| `Show-ServiceDialog`, `Save-ServiceDialog` | `sm-gui` (form) + `sm-core::validation` | Validasi per tipe |
| `Show-ImportDialog` | `sm-gui` (modal) | Checklist |
| `Export/Import-ConfigUi` | `sm-config` + `sm-gui` | File dialog |
| `ProjectUpdate.ps1` | `sm-updater` | CLI + logika |
| `Store-GitCredential.ps1` | `sm credential store` | Opsional; tetap pakai GCM |

### 7.1 Operasi SCM yang Dibutuhkan (Win32)

| Operasi | API |
| --- | --- |
| Buka SCM | `OpenSCManagerW` |
| Cek ada | `OpenServiceW` + `QueryServiceStatusEx` |
| Status | `QueryServiceStatusEx` (`SERVICE_STATUS_PROCESS`) |
| Start/Stop | `StartServiceW` / `ControlServiceW` |
| Hapus | `DeleteService` |
| Startup | `ChangeServiceConfigW` (`SERVICE_AUTO_START`, `SERVICE_DEMAND_START`) |
| Delayed | `ChangeServiceConfig2W` (`SERVICE_CONFIG_DELAYED_AUTO_START_INFO`) |
| Dependensi | `ChangeServiceConfigW` (`lpDependencies`) |
| Enumerasi | `EnumServicesStatusExW` |
| Cek admin | `OpenProcessToken` + `GetTokenInformation` (`TokenElevation`) |
| Elevasi | `ShellExecuteW` dengan verb `runas` |

---

## 8. Detail GUI

### 8.1 Layout Utama

- **Menu**: Aplikasi (Import Service, Backup, Restore, Buka Folder, Buka Log, Keluar), Service (Tambah, Edit, Hapus, Install, Start/Stop/Restart, Start/Stop/Restart Semua, Refresh), Bantuan (Tentang).
- **Toolbar**: tombol aksi utama, mirip versi PowerShell.
- **Grid**: kolom Status, Nama Service, Display Name, Jenis, Application, Startup, Restart; selection per baris; double-click = edit; context menu.
- **Panel log**: label service terpilih, checkbox auto-refresh, tombol Muat ulang/Buka file, textbox monospace read-only.
- **Status bar**: pesan status + jumlah terdaftar/terpasang/berjalan.
- **Timer**: status 3 detik, log 2 detik (dapat dikonfigurasi).

### 8.2 Dialog Service

- Field umum: nama, display name, tipe, executable, working directory, startup, restart, delay, dependensi, stdout/stderr.
- Field khusus Apache: `apacheRoot`, `apacheConfig`.
- Field khusus PostgreSQL: `pgData`, `pgAccount`.
- Validasi real-time: nama wajib & unik, executable ada, path native valid, delay >= 1.
- Tombol: Simpan, Simpan & Install, Batal.
- Konfirmasi restart bila service sedang berjalan setelah edit.

### 8.3 Perilaku UX yang Dipertahankan

- Pesan error ramah pengguna, bukan stack trace mentah (stack trace masuk log).
- Konfirmasi aksi destruktif (hapus service, stop semua).
- Status `Not Installed` untuk config yang belum terpasang.
- Auto-refresh log tidak mengganggu posisi scroll pengguna.

---

## 9. Project Updater (sm-updater)

### 9.1 Perintah CLI

```
sm-updater                     # menu interaktif
sm-updater list
sm-updater check --project X
sm-updater check --all
sm-updater update --project X
sm-updater update --all
sm-updater --config PATH ...
```

Exit code: `0` sukses, `1` gagal sebagian/semua, `2` argumen tidak valid.

### 9.2 Algoritma

1. Muat config + validasi path tool.
2. Scan `scanRoots` + override manual.
3. Filter target (`--project`/`--all`).
4. Untuk setiap project:
   - cek working tree bersih (`git status --porcelain`);
   - cek branch aktif;
   - preflight `git ls-remote --heads origin` (timeout 30 detik);
   - `git fetch --prune origin` (timeout 120 detik);
   - bandingkan `HEAD..origin/<branch>`;
   - deteksi perubahan migration -> blokir Auto;
   - `git merge --ff-only`;
   - jalankan langkah kondisional (composer/npm/build/artisan/queue);
   - jalankan `postUpdateCommand`;
   - tulis log + kembalikan status.
5. Ringkas hasil dan exit code.

### 9.3 Status Hasil

`up-to-date`, `check`, `updated`, `dirty`, `blocked-migration`, `failed` (sama dengan versi PowerShell).

### 9.4 Aturan Langkah Kondisional

| Perubahan | Aksi |
| --- | --- |
| `composer.json`/`composer.lock` | `composer install --no-dev --optimize-autoloader --no-interaction` |
| `package-lock.json` | `npm ci` lalu `npm run build` |
| `package.json` | `npm install` lalu `npm run build` |
| `resources/*`, `vite.config.*`, `tsconfig.json` | `npm run build` |
| Cache Laravel ada | `php artisan optimize:clear` |
| Kode berubah + `queueRestart` | `php artisan queue:restart` |
| `database/migrations/*` | Blokir Auto |

### 9.5 Eksekusi Proses

- Gunakan `std::process::Command` dengan argumen array (tanpa `cmd /c`).
- Set env: `GCM_CREDENTIAL_STORE=dpapi`, `GCM_INTERACTIVE=never`, `GIT_TERMINAL_PROMPT=0`.
- Timeout eksplisit; kill process tree bila melewati batas.
- Tangkap stdout/stderr ke log; redaksi pola token/password.
- Jangan pernah menulis token ke log.

---

## 10. Keamanan

- **Tanpa secret di repo**: `.gitignore` tetap berlaku; config runtime tidak di-commit.
- **DPAPI**: jika perlu menyimpan rahasia sendiri, gunakan `CryptProtectData` dengan scope user + machine, simpan ciphertext saja.
- **Credential Git**: tetap melalui GCM; aplikasi tidak menyimpan PAT.
- **Elevasi**: cek token admin saat start; bila belum, elevasi via `ShellExecuteW runas` (atau manifest `requireAdministrator` pada build final).
- **Path/argumen**: selalu argumen array; validasi nama service (`[A-Za-z0-9._-]`), tolak path UNC/relatif untuk executable, canonicalize path.
- **Privilege**: service account default `NT AUTHORITY\NetworkService`; jangan jalankan sebagai `LocalSystem` tanpa alasan.
- **Log**: redaksi nilai yang mirip token (`ghp_`, `gho_`, `glpat-`, `Bearer`, `password=`).
- **Update aplikasi**: verifikasi signature installer (fase lanjutan).

---

## 11. Error Handling dan Logging

- Error domain: `ConfigError`, `ServiceError`, `NssmError`, `NativeServiceError`, `GitError`, `UpdateError`, `ElevationError`.
- Pesan UI dalam Bahasa Indonesia; detail teknis di `logs/app.log`.
- `app.log` dengan rotasi ukuran (mis. 5 MB x 3) dan timestamp lokal.
- Log updater per-run: `logs/project-update-YYYYMMDD-HHmmss.log` + salinan ke `storage/logs` project.
- Operasi service menunggu state akhir (`Running`/`Stopped`) dengan timeout dan pesan diagnostik (mis. `SERVICE_START_PENDING`).

---

## 12. Strategi Pengujian

### 12.1 Unit Test (`cargo test`)

- Parsing/serialisasi `services.json` dan `projects.json` (termasuk field hilang/unknown).
- Normalisasi default (startup, delay, displayName, tipe tidak dikenal).
- Topological sort start/stop, termasuk dependency cycle.
- Matriks keputusan update (file berubah -> langkah yang dijalankan).
- Validasi nama service/path.
- Redaksi log.

### 12.2 Integration Test (Windows-only, CI `windows-latest`)

- Buat service uji memakai executable dummy (mis. `cmd.exe /c ping -t 127.0.0.1`) melalui NSSM bila tersedia.
- Install -> start -> status -> stop -> hapus.
- Delayed start terpasang benar.
- Import service mendeteksi service uji.
- Config rusak -> backup otomatis + fallback kosong.
- Git test repo lokal (bare remote) untuk alur check/update/migration-block.

### 12.3 Manual Test Matrix

| Skenario | Harapan |
| --- | --- |
| Bukan admin | Prompt elevasi UAC muncul; batal = keluar dengan pesan |
| NSSM tidak ada | Peringatan; service native tetap bisa dikelola |
| Start service lambat | Menunggu timeout, pesan jelas, status akhir benar |
| Hapus service berjalan | Konfirmasi, stop, hapus, config ter-update |
| Config JSON rusak | Backup `services-broken-*.json`, aplikasi tetap jalan |
| Import ganda | Service yang sudah ada tidak diduplikasi |
| Update dengan migration | Dibatalkan sebelum pull |
| Remote auth gagal | Pesan kredensial, tidak ada perubahan file |
| Working tree kotor | Project dilewati, status `dirty` |
| Log besar | Tail dibatasi, UI tetap responsif |

### 12.4 Uji Kompatibilitas

- Jalankan Rust dan PowerShell pada `services.json` yang sama; bandingkan daftar service, status, dan hasil operasi.
- Simpan fixture config dari versi PowerShell sebagai golden sample.

---

## 13. Build, Distribusi, dan CI

- **Build**: `cargo build --release --workspace`; GUI dibundel Tauri.
- **Installer**: MSI/NSIS via Tauri bundler; sertakan shortcut dan manifest admin.
- **Portable**: ZIP berisi `sm-gui.exe`, `sm-updater.exe`, contoh config.
- **CI**: GitHub Actions `windows-latest`:
  - `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`;
  - build release + upload artifact;
  - opsional: code signing (sertifikat di secret CI).
- **Versioning**: SemVer; tampilkan versi di dialog Tentang.
- **Dependency**: `cargo audit` / `cargo deny` di CI.

---

## 14. Fase Implementasi

| Fase | Deliverable | Estimasi |
| --- | --- | --- |
| 0. Fondasi | Workspace, CI, model data, config read/write + backup, test golden | 3-5 hari |
| 1. Core SCM | `sm-win::scm`, status, start/stop, delayed start, dependency ordering | 4-6 hari |
| 2. NSSM & native | Install/uninstall/apply config NSSM, Apache, PostgreSQL | 4-6 hari |
| 3. GUI utama | Tauri shell, grid, status/timer, log viewer, dialog service, import, backup/restore | 7-10 hari |
| 4. Updater | `sm-updater` CLI, git flow, langkah kondisional, log, migration guard | 5-7 hari |
| 5. Hardening | Elevasi, error handling, redaksi log, manual test matrix, performance | 3-5 hari |
| 6. Rilis | Installer, dokumentasi, migrasi config, pilot di server | 2-4 hari |

**Total:** ±30-45 person-days (1 pengembang Rust berpengalaman).

### 14.1 Rencana Rollout

1. **Paralel**: Rust dipasang di folder terpisah, membaca `services.json` yang sama (mode read-only untuk daftar).
2. **Pilot**: gunakan Rust untuk start/stop/status harian; PowerShell sebagai cadangan.
3. **Cutover**: setelah 1-2 minggu tanpa insiden, jadikan Rust default, PowerShell diarsipkan.
4. **Rollback**: simpan build PowerShell terakhir + config backup; tidak ada perubahan format config.

---

## 15. Risiko dan Mitigasi

| Risiko | Dampak | Mitigasi |
| --- | --- | --- |
| Perbedaan perilaku SCM (pending/timeout) | Service gagal start/stop | Uji integration + timeout eksplisit + pesan diagnostik |
| WebView2 tidak tersedia di server | GUI tidak tampil | Deteksi runtime; dokumentasikan kebutuhan; fallback build `Slint` (fase lanjutan) |
| UAC/elevasi berbeda saat dijalankan via Task Scheduler | Operasi gagal | Deteksi token admin, pesan jelas, dukung `--elevated`/manifest |
| Credential Manager tidak tersedia untuk akun service | Update gagal | Panduan `Store-GitCredential`, preflight sebelum pull |
| Config lama tidak kompatibel | Kehilangan data | Golden test + backup otomatis + tidak mengubah format |
| Antivirus/code signing | Binary diblokir | Code signing, checksum rilis, dokumentasi whitelist |
| Scope creep (fitur baru) | Mundur dari jadwal | Patuhi non-tujuan fase awal |

---

## 16. Kriteria Penerimaan

- [ ] `services.json` dan `projects.json` versi 1 terbaca dan tertulis tanpa kehilangan data.
- [ ] Semua operasi service NSSM/Apache/PostgreSQL berhasil di server uji.
- [ ] Import service terpasang bekerja untuk ketiga tipe.
- [ ] Delayed start, restart on crash, dan urutan dependensi berfungsi.
- [ ] Log viewer menampilkan tail dan auto-refresh.
- [ ] Backup otomatis (maks. 20) dan restore config berfungsi.
- [ ] `sm-updater` lulus seluruh matriks keputusan update dan memblokir migration.
- [ ] Tidak ada kredensial/token di repositori maupun log.
- [ ] Aplikasi berjalan sebagai admin dan menangani UAC dengan benar.
- [ ] CI hijau: fmt, clippy, unit test, integration test.
- [ ] Dokumentasi build/install/migrasi tersedia.

---

## 17. Pertanyaan Terbuka

1. Apakah target minimum Windows 10 21H2 atau perlu Windows Server 2016? (menentukan WebView2/Tauri).
2. Apakah aplikasi harus berjalan tanpa WebView2 (memaksa GUI native `Slint`/`egui`)?
3. Apakah perlu code signing resmi dan siapa pemegang sertifikat?
4. Apakah `Store-GitCredential` tetap script terpisah atau dijadikan subcommand Rust?
5. Apakah perlu dukungan multi-user/RDP concurrent (config per-user atau machine-wide)?
6. Apakah updater perlu dijalankan sebagai Windows Service/Task Scheduler pada fase awal?

---

## 18. Referensi Kode Acuan

| Topik | Lokasi saat ini |
| --- | --- |
| Model & normalisasi service | `ServiceManager.ps1` fungsi `ConvertTo-ServiceConfig` |
| Urutan dependensi | `ServiceManager.ps1` fungsi `Get-ManagedServicesInStartOrder` |
| Operasi NSSM | `ServiceManager.ps1` fungsi `Invoke-Nssm`, `Apply-ServiceConfig` |
| Native Apache/PostgreSQL | `ServiceManager.ps1` fungsi `Get-NativeInstallPlan`, `Apply-NativeServiceConfig` |
| Import/discovery | `ServiceManager.ps1` fungsi `Get-ManagedServiceCandidates` |
| Delayed start | `ServiceManager.ps1` kelas C# `ServiceDelayHelper` |
| Updater Laravel | `ProjectUpdate.ps1` fungsi `Invoke-ProjectUpdate` |
| Kredensial Git | `Store-GitCredential.ps1` |
| Skema config | `docs/CONFIGURATION.md`, `docs/PROJECT-UPDATE.md` |
| Keamanan | `docs/SECURITY.md` |

---

## 19. Langkah Berikutnya

1. Setujui keputusan GUI (Tauri vs native) dan target Windows minimum.
2. Buat workspace `service-manager-rs` + CI.
3. Implementasi Fase 0-1 (config + SCM) dengan test.
4. Review hasil sebelum masuk Fase 2 (NSSM/native).
