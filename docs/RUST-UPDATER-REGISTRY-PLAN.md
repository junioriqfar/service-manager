# Rencana Registry + Trigger Update

Dokumen ini merancang evolusi `sm-updater` dari **scan folder + update manual** menjadi **registry aplikasi + update yang dipicu dari PC 143**. Fokusnya bukan polling setiap menit, tetapi satu trigger aman dari komputer Anda ke server 160.

- Status: **Phase A selesai** (registry, trigger, allowlist, lock, state, toolchain, service lifecycle, clone-on-first-run, health check, webhook), **Phase B selesai** (`sm-trigger`, GUI Updater, GUI Remote, wrapper Scheduled Task). Sisa backlog: update paralel (`maxConcurrentUpdates`), scheduler opsional, dan GUI client macOS/Linux.
- Basis: `sm-updater` + `sm-trigger` + GUI Updater (254 test workspace)
- Prinsip: trigger eksplisit, aman, idempoten, tanpa polling default
- Config target: `data/projects.json` versi 2 (kompatibel versi 1)
- **Server tetap Windows**; trigger client direncanakan cross-platform (Windows, macOS, Linux).

### Status Implementasi

| Fitur | Status | Catatan |
| --- | --- | --- |
| Registry v2 + migrasi v1 | Selesai | `registry.rs`, `app add/list/show/edit/remove/test` |
| Trigger `run --all` / `--app` / `--check` | Selesai | `trigger.rs`, lock `data/updater.lock` |
| Allowlist IP/CIDR dua lapis | Selesai | `ip.rs`, per-app override |
| State persisten | Selesai | `data/updater-state.json` |
| Deteksi toolchain | Selesai | Composer, npm/pnpm/yarn/bun, Go, static, custom |
| Service lifecycle | Selesai | Stop/start/restart via SCM, preflight elevated |
| Custom commands | Selesai | Program + args, `shell` hanya admin |
| Health check / notify | Selesai | HTTP via curl; webhook best-effort |
| `cloneOnFirstRun` | Selesai | `git clone --branch ... --single-branch` |
| `maxConcurrentUpdates` | Belum | Saat ini serial |
| Scheduler/polling | Tidak dipakai | Default `false` |
| `sm-trigger` cross-platform | Selesai | Windows/macOS/Linux, shell ke `ssh` |
| GUI tab Updater | Selesai | Registry CRUD, test, run/check, log |
| GUI tab Remote | Selesai | Host/user, key/password (DPAPI), wrapper, preview, test SSH, run streaming |
| Wrapper Scheduled Task elevated | Selesai | `scripts/Invoke-UpdaterTask.ps1` + action/installer |

Catatan implementasi:

- Lock bersifat **global per config**, bukan per app, agar satu trigger tidak bertabrakan.
- `check` tidak pernah stop/restart service dan tidak memerlukan hak Administrator untuk
  preflight kontrol service (hanya validasi nama).
- Migration selalu `block`; tidak ada opsi menjalankan migration otomatis.

---

## 1. Koreksi Alur Lama

Alur lama **bukan polling** (dan bukan trigger otomatis terjadwal). Yang terjadi:

1. Anda menjalankan trigger dari PC 143.
2. Trigger masuk ke server 160 melalui **SSH**.
3. `ProjectUpdate.ps1` membaca environment `SSH_CONNECTION` atau `SSH_CLIENT`.
4. IP client dibandingkan dengan `allowedClientIPs` di `projects.json`.
5. Jika diizinkan, script melakukan cek/fetch/update satu kali lalu selesai.

Jadi sifatnya **event-driven / manual-triggered**, bukan background loop. Otomatisasi yang dimaksud adalah otomatisasi langkah setelah trigger: `git pull`, Composer, npm/build, dan queue restart.

## 2. Tujuan

1. Aplikasi harus **didaftarkan** dulu: repo URL, local path, branch, dan policy update.
2. Anda memicu update dari client (Windows/macOS/Linux) ke server Windows.
3. Server mengecek semua app terdaftar dan hanya mengupdate yang berubah.
4. Tidak ada polling setiap menit secara default.
5. Trigger dari IP/CIDR yang tidak diizinkan ditolak (IP whitelist).
6. Tetap aman: working tree kotor/branch salah/migration baru tidak diupdate otomatis.
7. Status dan log dapat dilihat dari CLI/GUI.

## 3. Perbedaan dengan Kondisi Saat Ini

| Aspek | Sekarang | Target |
| --- | --- | --- |
| Penentuan project | Scan `scanRoots` | Registry aplikasi eksplisit |
| Repo URL | Tidak disimpan | Disimpan per app |
| Trigger | SSH manual ke script PowerShell | SSH manual ke `sm-updater run --all` (atau GUI) |
| Client trigger | Windows (PC 143) | Windows, macOS, Linux (`sm-trigger`) |
| Polling | Tidak ada | **Tetap tidak ada secara default** |
| IP allowlist | `SSH_CONNECTION`/`SSH_CLIENT` | Dipertahankan di Rust, mendukung CIDR |
| Update | Manual satu project/all | Satu trigger untuk semua app terdaftar |
| Status | Log per run | State persisten + riwayat |
| Ganti service | `queue:restart` | `queue:restart` + service restart opsional |
| GUI | Belum ada | Tab Updater, tombol Update All |

## 4. Arsitektur Target

```
Trigger client (Windows / macOS / Linux)
   │  SSH (key/agent atau password)
   │  sm-trigger --host 10.255.240.160 --all
   ▼
Server 160 (Windows)
   ├─ validasi IP SSH (allowedClientIPs / CIDR)
   ├─ baca registry data/projects.json
   ├─ lock global data/updater.lock
   ├─ untuk tiap app: cek Git → update jika berubah
   ├─ composer/npm/build/artisan/queue restart
   └─ tulis state + log
```

Tidak ada daemon/polling pada fase awal. Trigger bisa berupa:

- **trigger client cross-platform** (Windows/macOS/Linux) yang menjalankan SSH,
- SSH remote command langsung dari terminal mana pun,
- menjalankan `sm-updater` langsung di server,
- tombol **Update All** di GUI server (nanti).

Server hanya menerima perintah lewat SSH; tidak ada port HTTP baru yang dibuka.

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
    "bunPath": "C:\\Users\\EGOV22\\.bun\\bin\\bun.exe",
    "pnpmPath": "",
    "yarnPath": "",
    "goPath": "C:\\Program Files\\Go\\bin\\go.exe",
    "logDir": "C:\\tools\\ServiceManager\\logs",
    "allowedClientIPs": ["10.255.240.143", "127.0.0.1", "::1"],
    "maxConcurrentUpdates": 1
  },
  "trigger": {
    "allowedClientIPs": ["10.255.240.143", "10.255.240.0/24", "127.0.0.1", "::1"],
    "allowLocal": true
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
      "preset": "auto",
      "composer": "auto",
      "artisan": "auto",
      "packageManager": "auto",
      "packageInstall": "auto",
      "build": "auto",
      "installCommand": "",
      "buildCommand": "",
      "queueRestart": true,
      "services": {
        "stopBeforePull": [],
        "stopBeforeBuild": ["opencode-quota"],
        "restartAfter": ["bsi-qris-api", "opencode-quota"],
        "startAfter": [],
        "waitTimeoutSeconds": 60
      },
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
| `preset` | Tidak | `auto`, `laravel`, `node`, `bun`, `go`, `static`, `custom` |
| `composer` | Tidak | `auto`, `always`, `never` |
| `artisan` | Tidak | `auto`, `always`, `never` |
| `packageManager` | Tidak | `auto`, `npm`, `pnpm`, `yarn`, `bun`, `none` |
| `packageInstall` | Tidak | `auto`, `always`, `never` |
| `build` | Tidak | `auto`, `always`, `never` |
| `installCommand` / `buildCommand` | Tidak | Override perintah manual (mis. Bun `build:win`, Go `make build-windows`) |
| `queueRestart` | Tidak | Restart queue setelah update |

### 5.2 Field Trigger

| Field | Default | Keterangan |
| --- | --- | --- |
| `trigger.allowedClientIPs` | `["127.0.0.1","::1"]` | IP/CIDR SSH yang boleh memicu |
| `trigger.allowLocal` | `true` | Izinkan trigger langsung di server tanpa SSH |
| `scheduler.enabled` | `false` | Polling hanya jika diaktifkan |
| `scheduler.checkIntervalMinutes` | `5` | Hanya dipakai jika scheduler aktif |
| `maxConcurrentUpdates` | `1` | Batas update paralel (jika banyak app) |

Format IP yang didukung:

- IP tunggal: `10.255.240.143`
- CIDR: `10.255.240.0/24`
- IPv6: `::1` atau `2001:db8::/32`

`autoUpdate` per app tidak diperlukan pada fase trigger. App yang `enabled` akan diproses saat `run --all`. Mode `check` tetap tersedia untuk dry-run.

### 5.3 Deteksi Toolchain dan Preset

Framework **bukan penentu utama**. Urutan keputusan:

1. Field eksplisit di registry (`preset`, `packageManager`, `buildCommand`, dst.).
2. Deteksi lockfile/manifest di folder project.
3. Preset framework sebagai default.
4. Default konservatif: jangan jalankan tool yang tidak terdeteksi.

Contoh: Laravel bisa memakai npm, pnpm, yarn, atau bun; Bun bisa menjadi runtime, package manager, sekaligus bundler.

#### Penanda Deteksi

| Penanda | Arti |
| --- | --- |
| `composer.json` / `composer.lock` | Composer |
| `artisan` | Laravel; aktifkan `optimize:clear` dan `queue:restart` |
| `package-lock.json` | npm |
| `pnpm-lock.yaml` | pnpm |
| `yarn.lock` | yarn |
| `bun.lock` / `bun.lockb` / `bunfig.toml` | Bun |
| `package.json` tanpa lockfile | Default npm, dengan warning |
| `go.mod` | Go |
| `scripts.build` di `package.json` | Ada perintah build standar |
| `build:win` / `build:web` | Build khusus platform/web; butuh `buildCommand` eksplisit |
| Tidak ada manifest | Static/no-build; hanya `git pull` |
| Beberapa lockfile sekaligus | **Ambiguous**; jangan menebak, minta config |

#### Hasil Observasi Project `D:\Projects`

| Project | Stack terdeteksi | Package manager | Build |
| --- | --- | --- | --- |
| `PHPProjects\fileshare` | Laravel 13 + Inertia React + Vite | npm (`package-lock.json`) | `npm run build` → `vp build` |
| `PHPProjects\kasirin` | Laravel 13 + Inertia React + Vite | npm | `npm run build` |
| `PHPProjects\open-po` | Laravel 13 + Inertia React + Vite | npm | `npm run build` |
| `BunProjects\opencode-quota` | Bun + TypeScript + Hono + React | bun (`bun.lock`) | `bun run build:win` untuk server, `bun run build:web` untuk web |
| `NodeProjects\bsi-qris-api` | Node.js >= 22 (CommonJS) | npm | Tidak ada build |
| `NodeProjects\wa-otp-server` | Node.js >= 20.9 | npm | Tidak ada build |
| `GoProjects\presensi-sidoarjo-server` | Go 1.26 + PostgreSQL | — | `make build-windows` atau `go build -o build/syncserver.exe .` |
| `PHPProjects\fortunagarment` | Static HTML | — | Tidak ada build |
| `PHPProjects\open-po\.opencode` | Tooling internal | — | Bukan app; abaikan |

Catatan:

- `fileshare` memakai script build `vp build` (Vite Plus), jadi updater **tidak boleh** memanggil `vite build` langsung; cukup `npm run build`.
- `opencode-quota` tidak punya script `build` biasa; build server memakai `build:win` dan build web memakai `build:web`.
- `bsi-qris-api` dan `wa-otp-server` tidak punya build script; install dependency saja.
- `presensi-sidoarjo-server` bukan Laravel/Node; gunakan `buildCommand` atau `postUpdateCommand`.
- `fortunagarment` hanya perlu `git pull`.
- Beberapa repo bisa sedang dirty; pipeline existing akan skip (status `dirty`).

#### Profil Registry yang Disarankan

| Profil | Field penting | Perintah |
| --- | --- | --- |
| Laravel + npm | `preset=laravel`, `composer=auto`, `packageManager=npm`, `build=auto` | composer install, `npm ci`, `npm run build`, artisan |
| Laravel + bun | `preset=laravel`, `composer=auto`, `packageManager=bun`, `build=auto` | composer install, `bun install --frozen-lockfile`, `bun run build`, artisan |
| Bun-only | `preset=bun`, `composer=never`, `packageManager=bun`, `buildCommand="bun run build:win"` | `bun install --frozen-lockfile`, build command |
| Node-only | `preset=node`, `composer=never`, `packageManager=npm`, `build=never` | `npm ci`; restart service |
| Go | `preset=go`, `composer=never`, `packageManager=none`, `buildCommand="make build-windows"` | build command; restart service |
| Static | `preset=static`, `composer=never`, `packageManager=none`, `build=never` | `git pull` saja |

Dengan model ini, aplikasi baru cukup diarahkan ke folder/repo; updater mendeteksi toolchain yang ada, lalu operator dapat mengoreksi lewat field eksplisit jika hasil deteksi ambigu.

### 5.4 Preset Custom, Custom Commands, dan Service Lifecycle

#### Preset Custom

`preset: "custom"` **bukan mengedit kode aplikasi**. Artinya operator menuliskan **command plan** secara manual di registry/GUI, karena project tidak cocok dengan preset otomatis.

Command plan memakai program + argumen array (bukan string shell) agar aman:

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
        "name": "build server",
        "program": "C:\\Program Files\\Git\\usr\\bin\\make.exe",
        "args": ["build-windows"],
        "timeoutSeconds": 600
      }
    ],
    "preUpdate": [],
    "postUpdate": []
  }
}
```

Field command:

| Field | Wajib | Keterangan |
| --- | --- | --- |
| `name` | Tidak | Label di log |
| `program` | Ya | Path executable |
| `args` | Tidak | Array argumen, bukan string shell |
| `cwd` | Tidak | Default: `localPath` |
| `timeoutSeconds` | Tidak | Default 300 |
| `env` | Tidak | Environment tambahan |
| `continueOnError` | Tidak | Default `false` |

Untuk perintah kompleks yang memang butuh shell, sediakan field `shell` eksplisit dan hanya boleh diisi admin. Jangan mengeksekusi string dari repo.

#### Service Lifecycle

Integrasi Service Manager memakai nama service Windows dari `data/services.json`.

```json
"services": {
  "stopBeforePull": [],
  "stopBeforeBuild": ["opencode-quota"],
  "restartAfter": ["bsi-qris-api", "opencode-quota"],
  "startAfter": [],
  "waitTimeoutSeconds": 60
}
```

| Field | Semantik |
| --- | --- |
| `stopBeforePull` | Stop service sebelum `git merge`; dipakai jika file yang di-track terkunci proses. |
| `stopBeforeBuild` | Stop service sebelum build; wajib untuk binary yang tidak bisa diganti saat service hidup. |
| `restartAfter` | Restart service setelah update/build. Jika service dihentikan oleh updater, service dinyalakan kembali. |
| `startAfter` | Pastikan service berjalan setelah update, meskipun sebelumnya stopped. |
| `waitTimeoutSeconds` | Batas tunggu state `Stopped`/`Running`. |

Aturan:

- `check`/dry-run **tidak pernah** stop/restart service.
- Service hanya disentuh jika ada perubahan yang benar-benar di-deploy.
- Jika `stopBeforeBuild` dijalankan lalu build gagal, updater **wajib mencoba menyalakan kembali** service yang tadi dihentikan (best-effort) dan mencatat status `failed`.
- Jika stop gagal (mis. bukan Administrator), update dibatalkan sebelum menyentuh file.
- Jika restart/start gagal setelah update, status menjadi `failed`/`degraded` dan notifikasi dikirim.
- Service yang tidak ada di `data/services.json` atau Windows SCM ditolak saat validasi.

Contoh penerapan:

| App | Stop sebelum build | Restart setelah |
| --- | --- | --- |
| `bsi-qris-api` | — | `bsi-qris-api` |
| `opencode-quota` | `opencode-quota` | `opencode-quota` |
| `presensi-sidoarjo-server` | `PresensiSidoarjoServer` | `PresensiSidoarjoServer` |
| Laravel queue | — | `queue:restart` (bukan service restart) |

#### Hak Akses

Stop/start/restart service membutuhkan **Administrator**. Trigger updater harus dijalankan:

- sebagai Administrator di server, atau
- sebagai akun service yang memiliki hak SCM, atau
- melalui `GlobalUpdater` yang dikonfigurasi elevated.

Jika tidak elevated, updater harus gagal **sebelum** mengubah file, bukan setelah build.

#### Rekomendasi Elevation untuk Trigger SSH

User SSH (`EGOV22`) belum tentu mendapat token Administrator. Karena itu service lifecycle sebaiknya dipisahkan:

1. SSH dari client menjalankan wrapper kecil sebagai `EGOV22`.
2. Wrapper membaca `SSH_CONNECTION`/`SSH_CLIENT`, memvalidasi IP/CIDR.
3. Jika valid, wrapper memicu **Windows Scheduled Task** `ServiceManager-Updater` yang dikonfigurasi **Run with highest privileges** sebagai `EGOV22`.
4. Task menjalankan `sm-updater run --all` dengan token elevated; DPAPI/GCM milik `EGOV22` tetap tersedia.
5. Firewall tetap membatasi port SSH hanya dari IP allowlist.

Alternatif: Windows Service `GlobalUpdater` berjalan sebagai akun privileged + SSH deploy key untuk Git. Ini lebih kompleks dan tidak perlu untuk fase awal.

Dengan pola Task Scheduler ini, IP whitelist tetap dicek di wrapper sebelum task elevated dijalankan.

## 6. Fitur dan Status Implementasi

Tabel pada bagian ini mencakup fitur Phase A yang sudah berjalan dan backlog berikutnya.
Status ringkas ada di bagian **Status Implementasi** di atas.

### 6.1 Registrasi Aplikasi

| Fitur | Deskripsi |
| --- | --- |
| Add app (CLI) | `sm-updater app add --id ... --repo ... --path ... --branch ...` |
| Add app (GUI) | Form di tab Updater |
| Edit/Remove/Enable | Kelola registry tanpa mengubah folder project |
| Test connection | `git ls-remote`, validasi path/branch/Laravel |
| Clone on first run | Clone repo jika `localPath` belum ada |
| Import dari scan | Ubah project hasil scan menjadi registry entry |
| Custom command plan | Install/build/pre/post command manual untuk preset custom |
| Service lifecycle | Atur stopBeforePull/stopBeforeBuild/restartAfter/startAfter |
| Validasi | ID unik, URL bersih, path tidak konflik, nama service ada |

### 6.2 Trigger dari Client (PC 143 dan Lainnya)

| Fitur | Deskripsi |
| --- | --- |
| SSH trigger | `sm-trigger` atau `sm-updater run --all` dari client |
| Cross-platform | Client Windows, macOS, Linux |
| IP allowlist | Baca `SSH_CONNECTION`/`SSH_CLIENT`, cocokkan IP/CIDR |
| Firewall | Windows Firewall membatasi port SSH hanya dari IP allowed |
| Local trigger | Tanpa env SSH (langsung di server) diizinkan jika `allowLocal` |
| Unauthorized | Exit code khusus (mis. `3`) + log IP ditolak |
| Auth | CLI: SSH key/agent atau password interaktif; GUI: key/agent atau password (DPAPI opsional) |
| Host key | Verifikasi `known_hosts` |
| Dry-run | `sm-trigger --all --check` |
| Single app | `sm-trigger --app kasirin` |
| Lock | Cegah dua trigger berjalan bersamaan |
| No-op | App tanpa perubahan tidak diubah |

### 6.3 Pipeline Update

Pipeline lama dipertahankan:

1. Validasi repo/path/branch.
2. **Resolusi toolchain** (explicit → lockfile → preset).
3. Cek working tree bersih.
4. Preflight auth remote + fetch.
5. Blokir migration.
6. **Stop service `stopBeforePull`** (jika dikonfigurasi dan akan merge).
7. `git merge --ff-only`.
8. Composer/package install jika terdeteksi.
9. **Stop service `stopBeforeBuild`** (hanya jika build akan dijalankan).
10. Build (`npm/bun/pnpm/yarn`, `buildCommand`, atau `make`).
11. `php artisan optimize:clear` + `queue:restart` jika Laravel.
12. **Restart/start service `restartAfter`/`startAfter`**.
13. Pre/post command.
14. Health check (opsional).
15. Simpan state + log + rollback hint.

Jika langkah 9 sudah menghentikan service lalu langkah 10 gagal, updater harus mencoba menyalakan kembali service tersebut di blok `finally` dan mencatat `failed`.

Perintah per toolchain:

| Toolchain | Install | Build |
| --- | --- | --- |
| Composer | `composer install --no-dev --optimize-autoloader --no-interaction` | — |
| npm | `npm ci` (lock) / `npm install` | `npm run build` (jika script ada) |
| pnpm | `pnpm install --frozen-lockfile` | `pnpm run build` |
| yarn | `yarn install --frozen-lockfile` | `yarn run build` |
| bun | `bun install --frozen-lockfile` | `bun run build` atau `buildCommand` |
| Go/custom | — | `buildCommand` (mis. `make build-windows`) |
| Static | — | — |

Jika `build` tidak ditemukan dan `build=auto`, lewati dengan log. Jika `build=always` tetapi perintah tidak ada, laporkan `failed`.

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
sm-updater app add --id kasirin --name Kasirin --repo https://... --path D:\... --branch main `
  --preset laravel --package-manager npm --build auto
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

### 6.6 GUI (Tab Updater dan Remote)

- Tiga tab: **Services**, **Updater**, **Remote**.
- Tab **Updater**:
  - Tabel app terdaftar: name, repo, branch, status, last check, last update.
  - Tombol: Add, Edit, Remove, Test, **Update All**, Check All, Refresh.
  - Form registrasi: repo URL, local path, branch, preset/toolchain, composer/build, queue restart,
    service lifecycle, allowlist IP, clone-on-first-run, health check, webhook.
  - Validasi nama service terhadap `data/services.json`.
  - Status badge + log.
- Tab **Remote**:
  - Field host, user, port, auth mode (key/password), identity, password + remember
    (DPAPI), wrapper, remote binary, remote config, app ID.
  - Preview perintah SSH, Test SSH (`BatchMode=yes` untuk key; `SSH_ASKPASS` untuk
    password), Check/Update All, Check/Run App.
  - Output stdout/stderr live via event Tauri.
  - Konfigurasi disimpan di `data/trigger-client.json` (tidak di-commit).
  - Tooltip pada aksi utama; klik kanan app Updater: Run/Check/Test/Log/Edit/Delete.
- Tidak ada background polling; semua aksi dipicu tombol/user.

### 6.7 Trigger Client Cross-Platform

Aplikasi trigger dipisahkan dari server agar bisa dibangun untuk Windows, macOS, dan Linux.

| Komponen | Platform | Isi |
| --- | --- | --- |
| `sm-updater` | **Windows saja** | Engine update, Git/Composer/npm, SCM service, PowerShell hooks |
| `sm-trigger` | Windows, macOS, Linux | Client SSH: Check All / Update All / app tunggal |
| `sm-gui` | **Windows saja** | Service Manager + tab Updater lokal server + tab Remote SSH client |

Fitur `sm-trigger`:

- CLI ringan tanpa dependensi Windows.
- Profil server: host, port, user, identity file, known_hosts.
- Menjalankan `ssh` sistem (OpenSSH Client di Windows 10/11, bawaan macOS/Linux).
- CLI tidak menyimpan password; password diketik interaktif jika server meminta.
- GUI Remote mendukung password dengan `SSH_ASKPASS` dan opsi Remember via DPAPI.
- Verifikasi host key; GUI memakai `accept-new` (TOFU) untuk host baru.
- Perintah:
  ```powershell
  sm-trigger --host 10.255.240.160 --user EGOV22 --all
  sm-trigger --host 10.255.240.160 --user EGOV22 --all --check
  sm-trigger --host 10.255.240.160 --user EGOV22 --app kasirin
  ```
- GUI Remote (Windows): tab **Remote** di `sm-gui` dengan Preview, Test SSH, dan output live.
- Opsional berikutnya: GUI client macOS/Linux dan output `--json` terstruktur.

Build target:

| OS | Artefak |
| --- | --- |
| Windows x64 | `.exe` (portable) / MSI |
| macOS universal | `.app` / DMG (build di macOS runner; signing/notarization opsional) |
| Linux x86_64 | AppImage / `.deb` |

CI matrix: `windows-latest`, `macos-latest`, `ubuntu-latest`. Server binary tetap hanya dibangun/diuji di Windows.

### 6.8 Opsional (Fase Lanjutan)

- `scheduler.enabled=true` + `run --due` via Task Scheduler.
- Windows Service `GlobalUpdater` (`daemon`).
- Webhook GitHub/GitLab (real-time, butuh endpoint publik).
- Notifikasi webhook dan health check.
- Rollback command/otomatis.
- Update window.

## 7. Alur Trigger

```
Trigger client (Windows / macOS / Linux)
  └─ ssh server160 "C:\tools\ServiceManager\target\release\sm-updater.exe run --all"
       ├─ cek SSH client IP (exact/CIDR)
       ├─ cek firewall/allowlist server
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

- IP allowlist dari `SSH_CONNECTION`/`SSH_CLIENT` (sama seperti perilaku lama), mendukung IP tunggal dan CIDR.
- **Windows Firewall**: port SSH hanya dibuka untuk IP trigger yang diizinkan (defense in depth).
- `repoUrl` tidak boleh berisi kredensial.
- Token tetap di GCM/DPAPI; config hanya menyimpan profil/host.
- Trigger lokal tanpa SSH diizinkan untuk admin server (`allowLocal`).
- `pre/postUpdateCommand` hanya dari config admin.
- Lock mencegah trigger ganda.
- Migration tetap manual.
- Stop/restart service membutuhkan Administrator; updater gagal sebelum mengubah file jika tidak elevated.
- Custom command memakai program + args, bukan string shell dari repo.
- Log di-redaksi, termasuk IP yang ditolak.
- Trigger client memverifikasi host key (`known_hosts`); GUI memakai `accept-new` (TOFU).
- Password tidak disimpan plaintext; GUI hanya menyimpan ciphertext DPAPI jika Remember aktif.
- Tidak ada HTTP trigger listener di server pada fase awal.

## 9. Migrasi Config

- v1 `projects.json` tetap dibaca.
- `sm-updater migrate-config` mengubah `projects`/`scanRoots` menjadi `apps`.
- Backup otomatis ke `backup/` sebelum menulis.
- App tanpa `repoUrl` tetap bisa update manual, tetapi tidak masuk registry sampai dilengkapi.

## 10. Fase Implementasi

| Fase | Deliverable | Estimasi |
| --- | --- | --- |
| A. Registry & trigger server | Schema v2, migrasi, toolchain detection (npm/bun/composer/go/static), custom commands, `app add/list/edit/remove/test`, `run --all`, IP/CIDR allowlist, lock, firewall docs, test | 5-7 hari |
| B. Trigger client cross-platform | `sm-trigger` CLI Windows/macOS/Linux + SSH profile + host key + status | 3-5 hari |
| C. GUI Updater server | Tab, tabel, form registrasi, test, Update All/Check All, status/log | 5-7 hari |
| D. Integrasi | Service lifecycle (stop sebelum build, restart setelah), pre/post hook, health check, webhook opsional | 4-6 hari |
| E. Opsional | GUI client macOS/Linux, Task Scheduler/polling, daemon, rollback | 3-5 hari |

**Total fase inti:** 17-25 person-days; fitur opsional 3-5 hari tambahan.

Status: **A-D selesai**. E sebagian: GUI client Windows (tab Remote) selesai;
GUI macOS/Linux dan scheduler/daemon belum.

## 11. Kriteria Penerimaan

- [ ] App harus terdaftar sebelum diproses `run --all`.
- [ ] Trigger dari PC 143 diizinkan hanya jika IP/CIDR ada di allowlist.
- [ ] Trigger dari IP lain ditolak dengan exit code jelas dan log.
- [ ] Windows Firewall membatasi port SSH hanya untuk IP trigger.
- [ ] Trigger tidak melakukan polling; hanya satu kali cek/update.
- [ ] App tanpa perubahan tidak diubah.
- [ ] Deteksi Laravel+npm, Bun-only, Node-only, Go, dan static berjalan sesuai profil.
- [ ] Preset `custom` menjalankan `commands.install/build/preUpdate/postUpdate` dengan program + args.
- [ ] Ambiguous lockfile (mis. npm + bun) ditolak dan minta config eksplisit.
- [ ] `buildCommand`/`installCommand` override dihormati.
- [ ] `stopBeforeBuild` menghentikan service sebelum build dan `restartAfter` menyalakannya kembali.
- [ ] Jika build gagal setelah service dihentikan, service dicoba dinyalakan kembali.
- [ ] Service stop/restart gagal jika updater tidak elevated, sebelum file diubah.
- [ ] Trigger SSH memvalidasi IP lalu memicu task elevated (Task Scheduler) atau mekanisme privileged yang setara.
- [ ] Working tree kotor/branch salah/migration baru tidak diupdate otomatis.
- [ ] Satu trigger memproses semua app enabled dengan lock.
- [ ] State/log per app tersedia di CLI dan GUI.
- [ ] Config v1 tetap terbaca; migrasi v2 aman + backup.
- [ ] `sm-trigger` dapat dibangun untuk Windows, macOS, dan Linux.
- [ ] Client memverifikasi host key; password hanya disimpan sebagai ciphertext DPAPI (opsional).
- [ ] Tidak ada token/password plaintext di config/log.

## 12. Non-Goals

- Polling setiap menit secara default.
- Menjalankan engine updater penuh di macOS/Linux (hanya trigger client).
- Membuka HTTP trigger listener tanpa kebutuhan.
- Menyimpan password SSH plaintext di client/config/log.
- Menjalankan kode/string shell arbitrer dari repo; custom command harus dideklarasikan admin.
- Auto-run migration.
- Merge conflict resolution otomatis.
- Menyimpan token di config.
- Deploy app yang belum terdaftar di registry.

## 13. Pertanyaan Terbuka

1. Trigger client cukup CLI, atau perlu GUI di Windows/macOS/Linux?
2. macOS: perlu code signing/notarization atau cukup build unsigned?
3. Apakah trigger harus bisa dijalankan tanpa login RDP (SSH key) — seperti sekarang?
4. Apakah `allowedClientIPs` perlu CIDR atau cukup IP tunggal?
5. Apakah ada lebih dari satu server tujuan (profil server di client)?
6. Apakah clone repo otomatis diperlukan untuk app baru?
7. Apakah `opencode-quota` harus build `build:win` (server exe) atau `build:web` (frontend)?
8. Apakah project Go (`presensi-sidoarjo-server`) cukup lewat `buildCommand: make build-windows`?
9. Apakah updater dijalankan sebagai Administrator, atau lewat service elevated?
10. Apakah service queue perlu direstart selain `php artisan queue:restart`?

## 14. Rekomendasi

1. **Pertahankan model trigger SSH** dari PC 143; jangan bangun polling.
2. Registry + `sm-updater run --all` + IP/CIDR allowlist + firewall + lock + service lifecycle sudah menjadi fitur inti.
3. `sm-trigger` CLI cross-platform sudah ada; GUI client Windows tersedia di tab **Remote**.
4. Server tetap Windows; engine updater tidak diport ke macOS/Linux.
5. Service lifecycle sudah aktif dan menggunakan preflight elevated.
6. Tab **Updater** dan **Remote** sudah tersedia di `sm-gui`.
7. Polling/daemon hanya jika nanti benar-benar dibutuhkan.

---

## Referensi

- Implementasi saat ini: [RUST-UPDATER.md](RUST-UPDATER.md)
- Inventaris fitur: [RUST-UPDATER-PLAN.md](RUST-UPDATER-PLAN.md)
- Service global/queue: [RUST-SERVICES.md](RUST-SERVICES.md)
- Keamanan: [RUST-SECURITY.md](RUST-SECURITY.md)
