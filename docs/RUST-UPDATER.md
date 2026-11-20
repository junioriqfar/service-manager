# Project Updater Rust (`sm-updater`)

CLI updater berbasis Rust untuk server Windows. Mendukung registry app multi-toolchain
(Laravel, Node/npm/pnpm/yarn, Bun, Go, static, custom), trigger SSH dengan allowlist IP/CIDR,
serta lifecycle Windows Service.

> Rencana lengkap: [RUST-UPDATER-REGISTRY-PLAN.md](RUST-UPDATER-REGISTRY-PLAN.md).
> Inventaris fitur: [RUST-UPDATER-PLAN.md](RUST-UPDATER-PLAN.md).

## Build

```powershell
cargo build --release -p sm-updater
# hasil: target\release\sm-updater.exe
```

## Quick Start Registry

```powershell
# Registrasi app + lifecycle service
sm-updater app add --id bsi-qris-api --name "BSI QRIS API" `
  --repo https://github.com/example/bsi-qris-api.git `
  --path D:\Projects\NodeProjects\bsi-qris-api --branch main `
  --preset node --package-manager npm --composer never `
  --package-install auto --build never `
  --restart-after bsi-qris-api

sm-updater app add --id opencode-quota --name "OpenCode Quota" `
  --repo https://github.com/example/opencode-quota.git `
  --path D:\Projects\BunProjects\opencode-quota --branch main `
  --preset bun --package-manager bun --composer never `
  --install-command "bun install --frozen-lockfile" `
  --build-command "bun run build:win" `
  --stop-before-build opencode-quota --restart-after opencode-quota

sm-updater app list
sm-updater app show bsi-qris-api
sm-updater app test bsi-qris-api
```

`app add`/`app edit` menerima opsi toolchain, command, allowlist IP per app, dan service
lifecycle. Gunakan `app edit ID ...` untuk mengubah app yang sudah terdaftar.

## Trigger

```powershell
sm-updater run --all
sm-updater run --all --check      # dry-run, tidak stop/restart service
sm-updater run --app kasirin
sm-updater run --app kasirin --check
sm-updater authorize               # validasi IP client (dipakai wrapper SSH)
sm-updater status
sm-updater logs kasirin --tail 100
```

Dari PC client melalui SSH:

```powershell
ssh EGOV22@10.255.240.160 "C:\tools\ServiceManager\target\release\sm-updater.exe run --all"
```

`run` dan `authorize` memakai trigger efektif: `settings.json.trigger` bila file ada,
selain itu `projects.json.trigger`. `--client-ip` hanya tersedia untuk `run`; wrapper
SSH memakai IP dari environment.

Exit code:

| Kode | Arti |
| --- | --- |
| `0` | Sukses / tidak ada kegagalan |
| `1` | Config tidak valid, app tidak ditemukan, atau ada update gagal |
| `2` | Argumen CLI tidak valid |
| `3` | `run --app` ditolak allowlist IP |

`run` memakai lock `data/updater.lock`; proses kedua akan ditolak sampai lock stale
(30 menit) atau proses pertama selesai. State terakhir disimpan di `data/updater-state.json`.

### Plan (dry-run)

```powershell
sm-updater plan --app kasirin
sm-updater plan --all
sm-updater plan --project kasirin      # alias --app untuk konsistensi
```

`plan` menampilkan rencana update **tanpa** merge, clone, build, install,
stop/restart service, menulis log, config, atau state. Satu-satunya perubahan
repositori adalah `git fetch --prune origin` yang memperbarui remote-tracking refs
di `.git`; `ls-remote` memakai timeout pendek dan tidak membuka prompt.

Urutan inspeksi:

1. Validasi branch/local path/Git. Folder project yang belum ada menghasilkan
   status `clone-required` (plan tidak melakukan clone).
2. `git status --porcelain`; entri difilter dengan `apps[].dirtyIgnore`. Jika masih ada
   entri yang tidak cocok, working tree `dirty` dan maksimal 30 entri blocking masuk
   warnings (fetch tidak dijalankan). Entri yang diabaikan tetap muncul sebagai warning
   `dirtyIgnore: ...`; bila tidak ada blocking, plan lanjut ke langkah berikutnya.
3. Verifikasi branch aktif sama dengan `branch` di registry, lalu baca HEAD.
4. `git ls-remote --heads origin`, `git fetch --prune origin`, verifikasi
   `origin/<branch>`, hitung `rev-list --count`, lalu baca `git log` dan
   `git diff --name-only`.
5. Tidak ada commit masuk → `up-to-date`; ada file
   `database/migrations/**` → `blocked-migration`; selain itu `ready`.
6. Toolchain di-resolve hanya untuk preview command; service hanya divalidasi
   (`exists`) dan dicek `is_running`. Health check/webhook hanya informasi dan
   webhook di-redaksi.
Output memuat status, branch/HEAD/remote HEAD, incoming commit, changed files,
migration files, command per group (`install`/`build`/`artisan`), service
stop/restart/start + state, health check, webhook, dan warnings.

Status plan: `up-to-date`, `dirty`, `blocked-migration`, `ready`,
`clone-required`, `error`.

Exit code `plan`:

| Kode | Arti |
| --- | --- |
| `0` | Plan berhasil dibuat, termasuk `ready`, `blocked-migration`, `clone-required`, atau `error` |
| `1` | Config/app gagal dimuat atau app tidak ditemukan |
| `2` | Argumen tidak valid (mis. tanpa `--app`/`--all`, atau keduanya) |

`plan` sengaja mengembalikan `0` untuk plan yang berisi status `error` karena
payload plan tetap berhasil dihasilkan; periksa `Status`/warnings untuk keputusan
operasional.

Batasan: plan tidak melakukan clone, tidak mengeksekusi health check/webhook, dan
belum memeriksa ancestry/fast-forwardability (`merge-base`). Status `ready`
mengasumsikan `git merge --ff-only` akan berhasil; bila branch lokal diverged,
update tetap bisa gagal meski plan menampilkan incoming commit.

### Dirty ignore (`apps[].dirtyIgnore`)

Pemeriksaan working tree memakai `git status --porcelain` yang difilter lewat
`sm_updater::dirty::filter_status`. Setiap entri dibandingkan dengan pola eksplisit
milik app (`apps[].dirtyIgnore`); tidak ada direktori artifact yang di-hardcode dan
`.gitignore` tidak diubah.

- Pola repo-relative, `/` dinormalisasi (Windows `\` diterima), case-insensitive.
- `*` dalam satu segmen, `**` lintas segmen, `?` satu karakter.
- Pola literal cocok dengan path itu sendiri dan turunannya; trailing `/` berarti
  direktori. Detail validasi ada di
  [CONFIGURATION.md](CONFIGURATION.md#dirty-ignore-appsdirtyignore).
- **All-or-nothing**: entri yang tidak cocok tetap memblokir. Run/check berhenti
  `dirty`, plan berstatus `dirty`, dan merge/install/build tidak dijalankan.
- Run/check menulis `INFO: N perubahan diabaikan oleh dirtyIgnore: ...` (maksimal
  10 entri ditampilkan) lalu daftar blocking (maksimal 10). Pesan `dirty` menyertakan
  jumlah blocking dan ignored. Tanpa pola, pesan lama
  `Working tree tidak bersih; project dilewati.` dipertahankan.
- Plan menambahkan warning `dirtyIgnore: <entry>` (maksimal 10 + jumlah sisanya).
  Bila hanya ada entri ignored, plan lanjut ke pengecekan branch/HEAD/fetch dan
  berstatus `up-to-date` atau `ready` sesuai commit masuk.
- Pola invalid ditolak oleh `validate_app` (GUI Save/Test/Preview Plan, serta CLI
  `app add/edit/test`). Filter runtime mengabaikan pola invalid secara defensif,
  sehingga pola tersebut tidak pernah menyembunyikan entri. Jangan memakai pola untuk
  menyembunyikan perubahan source, migration, config, atau file di luar repository.

### Wrapper Elevated (Scheduled Task)

Jika app memakai service lifecycle, jalankan updater melalui wrapper:

```powershell
.\scripts\Install-UpdaterTask.ps1 -UserId "$env:USERDOMAIN\$env:USERNAME"  # Administrator
```

Dari client lain:

```powershell
sm-trigger --host 10.255.240.160 --user EGOV22 --all `
  --wrapper "C:\tools\ServiceManager\scripts\Invoke-UpdaterTask.ps1"
```

Wrapper memvalidasi IP lewat `sm-updater authorize`, lalu menjalankan Scheduled Task
`ServiceManager-Updater` dengan **Run with highest privileges**. Task meneruskan client
IP ke `run --client-ip` agar allowlist per-app tetap berlaku. Lihat
[docs/RUST-TRIGGER.md](RUST-TRIGGER.md).

### GUI

Tab **Updater** di `sm-gui` menyediakan:

- daftar app + status terakhir;
- Add/Edit/Remove/Test app;
- Preview Plan (dry-run, read-only) dengan dialog status/commit/file/command/service;
- Check All / Update All dan run per app. **Check All** mengikuti
  `maxConcurrentUpdates` (stage 1: check-only paralel); **Update All** tetap serial.
- form toolchain, service lifecycle, allowlist per app, clone-on-first-run,
  health check, dan webhook;
- saran `webServerService` dari DocumentRoot Apache di tab Settings, dengan
  konfirmasi sebelum menulis;
- log updater terbaru.

Tab **Remote** menyediakan trigger SSH dari mesin client Windows:

- host/user/port, autentikasi key/agent atau password (opsional Remember via DPAPI),
  identity/wrapper/remote binary/remote config;
- Preview perintah, Test SSH, Check/Update All, Check/Run App;
- output stdout/stderr live;
- konfigurasi di `data/trigger-client.json` (password plaintext tidak disimpan).

Tab **Updater** mendukung klik kanan pada baris app: Preview Plan, Run, Check, Test, Log, Edit, Delete.
Beberapa tombol utama memiliki tooltip.

Tidak ada polling; semua aksi dipicu tombol.

## Global Settings (`data/settings.json`)

Path toolchain dan default updater dibaca dari `data/settings.json` bila file ada.
Template: [settings.example.json](../settings.example.json); file runtime
`data/settings.json` tidak di-commit.

- `tools` menyimpan path Git, PHP, Composer, Node, npm, pnpm, yarn, Bun, Go,
  Apache (`root`/`config`), dan NSSM. `version`/`detectedAt` diisi deteksi tool.
- `updater` menyimpan `logDir`, `maxConcurrentUpdates`, timeout, dan retensi log.
- `trigger` menyimpan `allowedClientIPs` dan `allowLocal`; nilainya divalidasi sebagai
  IP/CIDR IPv4/IPv6. Jika `settings.json` ada, trigger ini authoritative.
- `privilegedExecution` menyimpan `mode`, `taskName`, `userId`, dan `logonType` untuk
  Scheduled Task di tab Settings.
- `variables` dapat berisi variabel admin tambahan untuk command. Built-in
  `PHP`, `PHP_HOME`, `GIT`, `COMPOSER`, `NPM`, `BUN`, `GO`, `APACHE`,
  `APACHE_HOME`, dan `APACHE_CONFIG` selalu tersedia dan menang atas nama yang sama.
- Path wajib: Git, PHP, Composer, npm. Tool lain opsional.
- Password/token plaintext ditolak saat load/save.
- Tab **Settings** di GUI dapat mengisi path tool, menjalankan Auto-detect/Test,
  mengatur default updater, mengedit trigger, serta Install/Repair/Remove/Test
  Scheduled Task.
- **Migrate Trigger** menyalin `projects.json.trigger` ke `settings.json.trigger`
  secara eksplisit; `projects.json` tidak diubah.
- Perubahan Apache di **Save Settings** membuat `data/apache-pending.json`; apply ke
  service Apache adalah aksi terpisah yang membutuhkan Administrator. Lihat
  [CONFIGURATION.md](CONFIGURATION.md#perubahan-apache-dan-servicesjson).

Jika `settings.json` belum ada, `sm-updater` memakai `settings`/`trigger` lama di
`projects.json` sebagai fallback **tanpa menulis** file baru. Begitu `settings.json`
ada, file itu authoritative untuk path toolchain, `logDir`, dan trigger allowlist;
migrasi eksplisit tersedia dari tab Settings.

## Registry v2 (`data/projects.json`)

```json
{
  "version": 2,
  "settings": {
    "gitPath": "C:\\Program Files\\Git\\cmd\\git.exe",
    "phpPath": "C:\\laragon\\bin\\php\\php.exe",
    "composerPath": "C:\\laragon\\bin\\composer\\composer.bat",
    "npmPath": "C:\\Program Files\\nodejs\\npm.cmd",
    "bunPath": "C:\\Users\\EGOV22\\.bun\\bin\\bun.exe",
    "pnpmPath": "",
    "yarnPath": "",
    "goPath": "C:\\Program Files\\Go\\bin\\go.exe",
    "logDir": "C:\\tools\\ServiceManager\\logs",
    "maxConcurrentUpdates": 1
  },
  "trigger": {
    "allowedClientIPs": ["10.255.240.143", "10.255.240.0/24", "127.0.0.1", "::1"],
    "allowLocal": true
  },
  "apps": [
    {
      "id": "bsi-qris-api",
      "name": "BSI QRIS API",
      "enabled": true,
      "repoUrl": "https://github.com/example/bsi-qris-api.git",
      "localPath": "D:\\Projects\\NodeProjects\\bsi-qris-api",
      "branch": "main",
      "preset": "node",
      "composer": "never",
      "packageManager": "npm",
      "packageInstall": "auto",
      "build": "never",
      "webServerService": "Apache2.4",
      "services": {
        "stopBeforePull": [],
        "stopBeforeBuild": [],
        "restartAfter": ["bsi-qris-api"],
        "startAfter": [],
        "waitTimeoutSeconds": 60
      }
    }
  ]
}
```

`projects.json` v1 (Laravel-only) tetap dibaca otomatis dan dimigrasikan di memori ke v2.
Field v1 yang tersedia: `scanRoots`, `allowedClientIPs`, `gitPath`, `phpPath`,
`composerPath`, `npmPath`, `logDir`, `projects`.

### Field App Penting

| Field | Nilai | Keterangan |
| --- | --- | --- |
| `preset` | `auto`, `laravel`, `node`, `bun`, `go`, `static`, `custom` | `auto` mendeteksi dari manifest |
| `composer` | `auto`, `always`, `never` | Composer install |
| `artisan` | `auto`, `always`, `never` | `optimize:clear` dan `queue:restart` |
| `packageManager` | `auto`, `npm`, `pnpm`, `yarn`, `bun`, `none` | Lockfile ganda wajib eksplisit |
| `packageInstall` | `auto`, `always`, `never` | Install dependency Node/Go |
| `build` | `auto`, `always`, `never` | Build command |
| `installCommand` / `buildCommand` | string | Override perintah install/build (PowerShell); mendukung `${NAME}` |
| `preUpdateCommand` / `postUpdateCommand` | string | Shell command sebelum/sesudah merge; mendukung `${NAME}` |
| `commands.install/build/preUpdate/postUpdate` | array command | Program + args/cwd/shell/env; seluruh field mendukung `${NAME}` |
| `cloneOnFirstRun` | bool | Clone repo jika `localPath` belum ada |
| `webServerService` | string | Nama service Apache di `services.json` yang menyajikan app; kosong = tidak terhubung eksplisit |
| `healthCheck` | object | HTTP check setelah service restart |
| `notify` | object | Webhook best-effort untuk status terpilih |
| `migrationPolicy` | `block` | Migration tidak pernah dijalankan otomatis |

Tabel **Updater** menampilkan versi PHP efektif dari `settings.tools.php.version`
untuk preset `laravel`, serta service Apache (`webServerService`) dan versi
`settings.tools.apache.version` bila diisi.

### Propagasi `webServerService`

`webServerService` menghubungkan app ke service Apache di `data/services.json`.
Saat path/root/config/version Apache diubah di tab **Settings**:

- service Apache terdampak dicocokkan dari executable/root lama (atau semua service
  Apache untuk konfigurasi pertama);
- hanya app dengan `webServerService` yang menunjuk salah satu service terdampak
  yang ditampilkan di preview;
- settings disimpan lebih dulu, lalu service/app terdampak menunggu tombol
  **Apply Now** (butuh Administrator);
- apply memperbarui `services.json` + Apache `binPath` di Windows SCM, dengan
  backup, rollback, dan audit log. Detail ada di
  [CONFIGURATION.md](CONFIGURATION.md#perubahan-apache-dan-servicesjson).

Field ini tidak mengubah lifecycle updater app (stop/restart/start service tetap
memakai `services.*`).

### Deteksi `webServerService` dari DocumentRoot

GUI tab Settings menyediakan **Apache App Mapping** (Sprint 2.3):

- `detect_apache_mappings` membaca config Apache read-only, termasuk
  `ServerRoot`, `Define`/`${VAR}`, `Include`/`IncludeOptional`, wildcard, dan
  include bersarang dengan batas kedalaman/file;
- `DocumentRoot` dibandingkan dengan `apps[].localPath` secara case-insensitive:
  path sama (`exact`) dan DocumentRoot di bawah localPath (`descendant`) bernilai
  high, sedangkan localPath di bawah DocumentRoot (`ancestor`) bernilai medium;
- app dengan `webServerService` yang sudah terisi dikembalikan sebagai
  `alreadyMapped` dan tidak disarankan untuk ditimpa;
- `apply_apache_mappings` hanya menulis `webServerService` untuk app yang dipilih
  setelah konfirmasi, setelah membackup `backup/projects-<timestamp>.json`;
- tidak ada perubahan otomatis pada `projects.json` dari hasil deteksi.

Parser tidak mengevaluasi `<IfModule>` atau kondisi runtime Apache; semua
`DocumentRoot` yang terlihat di config/include akan dianalisis.

### Variabel command dan scan PHP hardcoded

Command app mendukung placeholder `${NAME}`:

| Variabel | Nilai |
| --- | --- |
| `PHP` | `settings.tools.php.path` |
| `PHP_HOME` | Folder induk path PHP |
| `GIT` / `COMPOSER` / `NPM` / `BUN` / `GO` | Path tool dari settings |
| `APACHE` | `settings.tools.apache.path` |
| `APACHE_HOME` | `settings.tools.apache.root` |
| `APACHE_CONFIG` | `settings.tools.apache.config` |

`settings.json.variables` menambah variabel admin; built-in menang jika namanya
sama. Ekspansi rekursif maksimal 8 level, placeholder `${NAME}` yang tidak dikenal
dibiarkan, dan environment variable tidak dibaca. Yang diekspansi: `installCommand`,
`buildCommand`, `preUpdateCommand`, `postUpdateCommand`, serta `program`, `args[]`,
`cwd`, `shell`, dan value `env` pada `commands.*`. Built-in path/args updater tidak
diubah.

Jika `tools.php.path` berubah saat **Save Settings**, backend memindai command app
untuk path/root lama:

- field yang dipindai: `installCommand`, `buildCommand`, `preUpdateCommand`,
  `postUpdateCommand`, dan `commands.*`;
- pencocokan case-insensitive serta menganggap `/` dan `\` sama;
- field yang sudah memakai `${PHP}`/`${PHP_HOME}` dilewati;
- hasilnya dikembalikan sebagai `phpPathChange` (app/field/value) dan **tidak**
  mengubah `projects.json`.

Rewrite hanya lewat command eksplisit `replace_php_references`:

- mode `variable`: path lama → `${PHP}`, root lama → `${PHP_HOME}`;
- mode `new_path`: path/root lama → path/root baru;
- hanya app yang direferensikan, dengan backup
  `backup/projects-<timestamp>.json` dan save atomik;
- sisa string dipertahankan.

## Deteksi Toolchain

Urutan keputusan:

1. Field eksplisit di registry.
2. Lockfile/manifest project.
3. Preset framework.
4. Default konservatif: tool yang tidak terdeteksi tidak dijalankan.

| Penanda | Toolchain |
| --- | --- |
| `artisan` + `composer.json` | Laravel |
| `go.mod` | Go |
| `package.json` + `bun.lock` / `bun.lockb` | Bun |
| `package.json` + `package-lock.json` | Node/npm |
| `package.json` + `pnpm-lock.yaml` | Node/pnpm |
| `package.json` + `yarn.lock` | Node/yarn |
| `package.json` tanpa lockfile | Node/npm (default) |
| Beberapa lockfile sekaligus | **Error**; isi `packageManager` eksplisit |
| Tanpa manifest | Static; hanya `git pull` |

Build otomatis hanya berjalan bila ada `scripts.build` dan file frontend/konfigurasi
berubah. Untuk script non-standar (`build:win`, `build:web`, `make build-windows`),
isi `buildCommand` eksplisit.

### Preset Custom

`preset=custom` memakai command plan admin, bukan kode dari repo:

```json
{
  "preset": "custom",
  "commands": {
    "install": [
      {
        "name": "bun install",
        "program": "C:\\Users\\EGOV22\\.bun\\bin\\bun.exe",
        "args": ["install", "--frozen-lockfile"],
        "timeoutSeconds": 300
      }
    ],
    "build": [
      {
        "name": "build windows",
        "program": "C:\\Program Files\\Git\\usr\\bin\\make.exe",
        "args": ["build-windows"],
        "timeoutSeconds": 600
      }
    ]
  }
}
```

Command memakai `program` + `args` (tanpa shell) dan `continueOnError` opsional.
Field `shell` hanya diisi admin untuk perintah yang memang butuh PowerShell.
Tidak ada perintah yang dibaca dari file repo.

## Service Lifecycle

```json
"services": {
  "stopBeforePull": [],
  "stopBeforeBuild": ["opencode-quota"],
  "restartAfter": ["opencode-quota"],
  "startAfter": [],
  "waitTimeoutSeconds": 60
}
```

| Field | Semantik |
| --- | --- |
| `stopBeforePull` | Stop sebelum `git merge` |
| `stopBeforeBuild` | Stop sebelum install/build |
| `restartAfter` | Restart setelah update; service yang tadi dihentikan akan dinyalakan |
| `startAfter` | Pastikan running setelah update, meski sebelumnya stopped |
| `waitTimeoutSeconds` | Batas tunggu state SCM |

Aturan:

- `check`/dry-run tidak pernah stop/restart service.
- Service yang sudah stopped sebelum update tidak dinyalakan kecuali diminta
  `restartAfter`/`startAfter`.
- Stop/start memerlukan Administrator. Preflight dilakukan untuk semua app terpilih
  sebelum Git disentuh; jika tidak elevated, seluruh run dibatalkan.
- `run --app` untuk app tanpa service tetap bisa dijalankan non-admin.
- Jika build/post-update gagal setelah service dihentikan, updater melakukan best-effort
  start dan menandai update `failed`.
- Jika service gagal dipulihkan setelah update sukses, status `degraded`.

## Health Check dan Notifikasi

```json
"cloneOnFirstRun": false,
"healthCheck": {
  "enabled": true,
  "url": "http://127.0.0.1:8100/health",
  "timeoutSeconds": 10
},
"notify": {
  "webhookUrl": "https://hooks.example.com/updater",
  "on": ["failed", "degraded", "updated"]
}
```

- `cloneOnFirstRun: true` menjalankan `git clone --branch <branch> --single-branch`
  jika `localPath` belum ada. Clone pertama langsung menjalankan install/build.
  Mode `check` tidak pernah clone.
- Health check memakai `curl.exe` (`-fsS --max-time N`) setelah service restart.
  Gagal health check → status `degraded` (file sudah ter-update).
- Webhook hanya dikirim jika status ada di `notify.on`, payload JSON
  `{app,status,message,time}`. Kegagalan webhook hanya warning, tidak membatalkan update.

## Alur Update

1. Validasi app, branch, service (termasuk hak akses); clone jika `cloneOnFirstRun`.
2. Cek working tree (`git status --porcelain`) dengan filter `dirtyIgnore`. Entri
   yang tidak cocok membuat status `dirty`; bila semua entri cocok, update lanjut.
3. Validasi branch aktif.
4. Preflight `git ls-remote --heads origin`.
5. `git fetch --prune origin`.
6. Bandingkan `HEAD..origin/<branch>`.
7. Deteksi perubahan `database/migrations/*` → `blocked-migration` sebelum merge.
8. Resolve toolchain dari file yang akan berubah (sebelum menyentuh file).
9. Jalankan `commands.preUpdate`, lalu `stopBeforePull`.
10. `git merge --ff-only origin/<branch>`.
11. `stopBeforeBuild`, install, build, artisan, `commands.postUpdate`.
12. Restart/start service sesuai plan.
13. Health check (jika aktif) dan webhook notifikasi.
14. Tulis log + rollback hint (`git reset --hard <sha-sebelum>`).

Migration tidak pernah dijalankan otomatis.

## Konkurensi Check-Only (Sprint 3.5 Stage 1)

`maxConcurrentUpdates` di `settings.json.updater` (validasi 1–32, default 1)
hanya berlaku untuk **check-only** pada stage 1:

- Jalur paralel dipakai oleh GUI **Check All**, `sm-updater run --all --check`,
  `sm-updater run --app ID --check`, dan trigger check. Tidak ada command baru.
  Mode legacy `sm-updater check`/menu `update_project` tetap serial.
- Syarat: nilai > 1 dan minimal dua app lolos filter enabled + filter `--app` +
  allowlist IP. Worker pool dibatasi `min(maxConcurrentUpdates, jumlah app allowed)`.
- App forbidden tetap dicatat sebagai `forbidden` tanpa memanggil `update_app`.
- Worker hanya menjalankan `update_app(check_only=true)`. State, `lastCommit`,
  history, dan urutan `RunOutcome` ditulis di thread utama setelah worker selesai,
  berurutan sesuai config dan tepat satu entry per app. Jika hasil worker hilang,
  app mendapat status `failed`.
- Check-only tidak pernah stop/start service, install, build, clone, atau merge.
- **Run/Update All, trigger update, install, build, clone, dan service lifecycle
  tetap serial** berapa pun nilai `maxConcurrentUpdates`. Per-app lock, batas
  restart service bersamaan, progress per app, dan rollback terisolasi menunggu
  stage 2.

## Allowlist IP

Dua lapis:

- `trigger.allowedClientIPs` untuk semua app. Sumbernya `settings.json.trigger` bila
  file ada; jika belum, fallback ke `projects.json.trigger`.
- `app.allowedClientIPs` (jika diisi) menggantikan allowlist global untuk app tersebut.

Format: IP tunggal (`10.255.240.143`), CIDR (`10.255.240.0/24`), atau IPv6 (`::1`,
`2001:db8::/32`). Pola invalid/kosong ditolak saat load/save `settings.json`. IP
client dibaca dari `SSH_CONNECTION`/`SSH_CLIENT`. Trigger lokal tanpa SSH diizinkan
bila `trigger.allowLocal=true`. `sm-updater authorize` memakai trigger efektif yang
sama dengan `sm-updater run`.

## Status

`up-to-date`, `check`, `updated`, `dirty`, `blocked-migration`, `degraded`, `failed`,
`forbidden`.

## Trigger Client

`sm-trigger` (Windows/macOS/Linux) memicu updater lewat SSH tanpa polling:

```bash
cargo build --release -p sm-trigger
sm-trigger --host 10.255.240.160 --user EGOV22 --all --check
```

Panduan lengkap: [docs/RUST-TRIGGER.md](RUST-TRIGGER.md).

## History (audit)

Setiap run registry menulis history append-only ke
`data/updater-history.jsonl` (sibling `projects.json`; satu JSON object per baris,
tanpa pretty print). Entry ditulis untuk **setiap app yang diproses**, termasuk
`forbidden`, `up-to-date`, `check`, `updated`, `dirty`, `blocked-migration`,
`degraded`, dan `failed`. App disabled, app tidak terpilih, dan plan dry-run tidak
menulis history.

Field entry (serde camelCase):

| Field | Isi |
| --- | --- |
| `timestamp` | Waktu lokal ISO 8601 saat entry ditulis |
| `appId` | ID app registry |
| `status` | Status outcome (`updated`, `forbidden`, dst.) |
| `message` | Pesan outcome, di-redaksi `sm_core::redact_secrets`, maksimal 4000 karakter |
| `commit` | HEAD best-effort setelah update; kosong bila Git/HEAD tidak tersedia atau app forbidden |
| `durationMs` | Durasi update app dalam milidetik |
| `clientIp` | IP client hasil resolusi trigger (`--client-ip` atau environment SSH); kosong untuk trigger lokal |

`load_updater_history(app_id?, limit)` (GUI) membaca newest-first, memfilter app
secara case-insensitive, mengabaikan baris rusak, dan meng-clamp `limit` ke
`1..=500` (default GUI 50). History tidak pernah menghapus atau menggagalkan update:
jika append gagal, error hanya dilaporkan ke stderr dan outcome run tidak berubah.
History bersifat append-only dan **tidak** dipangkas oleh retensi log; belum ada
rotasi/cap ukuran file.

## Log

- `logDir/project-update-<stamp>-<slug>-<id>.log` untuk app registry
  (`update_app`); mode legacy `update_project` tetap memakai
  `project-update-YYYYMMDD-HHmmss.log`.
- `<project>/storage/logs/project-update-<stamp>-<slug>-<id>.log` (hanya jika project
  sudah punya folder `storage`, mis. Laravel), dengan suffix yang sama seperti run log.
- Suffix `<slug>` adalah nama folder project yang disanitasi dan `<id>` adalah nomor
  urut atomik, sehingga dua app yang dicek/diupdate pada detik yang sama tidak
  berbagi file. Prefix retention `project-update-` tetap sama.

`sm-updater logs` menampilkan log global terbaru; `sm-updater logs ID --tail N`
menampilkan tail log project. Log berisi perintah, output (maksimal 60 baris per perintah),
commit/file berubah, dan rollback hint.

## Retensi Log

Retensi bersifat **manual-only**; update/run tidak pernah memangkas log otomatis.
Dari GUI Settings, **Clean Logs Now** memanggil `prune_logs_now` dengan aturan:

- Hanya file langsung di `logDir` yang namanya `project-update-*.log` atau
  `updater-task-*.log` (case-insensitive); folder/symlink dan file lain diabaikan.
- Log project hanya `project-update-*.log` di `<localPath>/storage/logs` untuk app
  yang terdaftar; path kosong/tidak valid dilewati.
- File dengan modified time lebih lama dari `logRetentionDays` dihapus; file yang
  dimodifikasi dalam 5 menit terakhir selalu dihitung `skippedActive` dan tidak
  dihapus.
- `days = 0` berarti retention dinonaktifkan dan command GUI mengembalikan error
  `Log retention is disabled (0 days).`
- `PruneResult` melaporkan `cutoff`, `scannedFiles`, `deletedFiles`,
  `deletedBytes`, `skippedActive`, dan `errors`. Error metadata/delete dicatat dan
  tidak menghentikan file lain; folder yang hilang menghasilkan hasil kosong.
- Tidak butuh Administrator. History, `app.log`, dan audit Apache
  `logs/apache-apply.log` tidak disentuh.

## Simpan Kredensial Git

```powershell
sm-updater credential store `
  --host gitlab.com `
  --username oauth2 `
  --project D:\Projects\PHPProjects\open-po `
  --git C:\laragon\bin\git\cmd\git.exe
```

Token diminta tanpa echo, dikirim ke `git credential approve` melalui stdin, lalu
diverifikasi dengan `git ls-remote --heads origin`. Token tidak pernah ditulis ke log/file.

Helper `git credential reject` tersedia untuk menghapus kredensial (`credential_reject_input`,
`reject_credential`) tanpa mengirim password. GUI Settings memakai helper ini untuk
**Clear**; token juga tidak disimpan di `settings.json`, history, atau nilai balik command.
Validasi host/username/token menolak baris baru/karakter kontrol untuk mencegah injeksi
protokol credential.

## Keamanan

- Env otomatis: `GCM_CREDENTIAL_STORE=dpapi`, `GCM_INTERACTIVE=never`, `GIT_TERMINAL_PROMPT=0`.
- Preflight autentikasi sebelum menyentuh working tree.
- Tidak ada token/password di log atau config; log di-redaksi `sm_core::redact_secrets`.
- Command registry dijalankan dengan argumen array; string shell hanya dari field admin.
- Tidak ada eksekusi kode/command yang dibaca dari repository.
- Timeout eksplisit: preflight 30 detik, fetch/merge 120 detik, tool default 300 detik.
- Lock updater mencegah dua proses berjalan bersamaan.

## Pengujian

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Test mencakup registry v1→v2, migrasi, allowlist IP/CIDR, lock, state, deteksi toolchain
(termasuk lockfile ambigu), custom command, service lifecycle, rollback service saat build
gagal, seluruh matriks update Laravel, serta plan dry-run (status up-to-date/ready/dirty/
blocked-migration/clone-required/error, command per group, service read-only, redaksi
webhook, parse CLI, dan smoke test tanpa SCM/network). History diuji untuk roundtrip
JSONL, urutan/limit, filter case-insensitive, baris rusak, file hilang, redaksi/cap
pesan, satu object per baris, integrasi trigger (allowed + forbidden + client IP),
dan kegagalan tulis yang tidak menggagalkan run. Retensi diuji untuk pola file,
batas hari, file aktif, folder hilang, log project, merge count, dan error delete
yang tidak menghentikan file lain. Maintenance mode diuji untuk default/normalize/validasi
message, penolakan update non-check tanpa state/history, dan check yang tetap berjalan.
Export/load settings diuji untuk roundtrip, legacy fallback, penolakan JSON/settings/
control character/non-`.json`, dan tidak ada write ke config aktif. Credential reject,
validasi host/username/token, dan redaksi token diuji tanpa menyentuh Credential Manager
asli. Check-only concurrency diuji dengan barrier dua worker (max=2), serial
saat max=1, update non-check tetap serial, serta urutan state/history. Total
workspace: 491 test.

## Task Scheduler / Elevation

Service lifecycle membutuhkan token elevated. Pola yang dipakai:

1. SSH menjalankan wrapper kecil sebagai `EGOV22`.
2. Wrapper memvalidasi `SSH_CONNECTION`/`SSH_CLIENT`.
3. Wrapper memicu Scheduled Task `ServiceManager-Updater` dengan **Run with highest privileges**.
4. Task menjalankan `sm-updater run --all`; DPAPI/GCM milik `EGOV22` tetap tersedia.

Jangan mengaktifkan polling. Trigger tetap manual dari client.
