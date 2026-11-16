# Konfigurasi Service Manager

Dokumen ini menjelaskan format config runtime yang dipakai aplikasi Rust (`sm-gui`,
`sm-updater`, `sm-trigger`): service Windows, registry project, dan konfigurasi Remote
Trigger.

## Lokasi File

| Item | Path |
| --- | --- |
| Config service | `<folder config>\data\services.json` |
| Config project | `<folder config>\data\projects.json` |
| Config settings global | `<folder config>\data\settings.json` |
| Config Remote Trigger | `<folder config>\data\trigger-client.json` |
| State updater | `<folder config>\data\updater-state.json` |
| Lock updater | `<folder config>\data\updater.lock` |
| Override folder | Environment variable `SERVICE_MANAGER_HOME` |
| Backup otomatis | `<folder config>\backup\services-*.json` |
| Export backup | `<folder config>\backup\services-export-*.json` |
| Log aplikasi | `<folder config>\logs\app.log` |
| NSSM | `..\nssm\nssm.exe` atau `C:\tools\nssm\nssm.exe` |

Jika `data/services.json` tidak ada, aplikasi dimulai dengan daftar kosong. Jika JSON tidak valid, file lama dicadangkan ke `backup/services-broken-<timestamp>.json` dan aplikasi tetap berjalan dengan daftar kosong.

## Struktur

```json
{
  "version": 1,
  "services": [
    {
      "name": "nama-service",
      "displayName": "Nama Tampilan",
      "serviceType": "nssm",
      "executable": "C:\\path\\app.exe",
      "arguments": "--port 3000",
      "workingDirectory": "C:\\path",
      "apacheRoot": "",
      "apacheConfig": "",
      "pgData": "",
      "pgAccount": "",
      "stdoutLog": "C:\\path\\logs\\out.log",
      "stderrLog": "C:\\path\\logs\\err.log",
      "startup": "auto",
      "restartOnCrash": true,
      "restartDelaySec": 5,
      "dependencies": ["Tcpip"],
      "addedAt": "2026-01-01T00:00:00+07:00",
      "updatedAt": "2026-01-01T00:00:00+07:00"
    }
  ]
}
```

## Field

| Field | Tipe | Wajib | Keterangan |
| --- | --- | --- | --- |
| `name` | string | Ya | Nama service Windows (unik, 1-256 karakter: huruf, angka, titik, `_`, `-`). |
| `displayName` | string | Tidak | Nama tampilan; default sama dengan `name`. |
| `serviceType` | string | Tidak | `nssm` (default), `apache`, atau `postgresql`. |
| `executable` | string | Ya | Path executable. Apache harus `httpd.exe`; PostgreSQL harus `pg_ctl.exe`. |
| `arguments` | string | Tidak | Argumen baris perintah untuk NSSM. |
| `workingDirectory` | string | Tidak | Direktori kerja proses. |
| `apacheRoot` | string | Apache | Root instalasi Apache (`ServerRoot`). |
| `apacheConfig` | string | Apache | Path config; relatif terhadap `apacheRoot` atau absolut. Default `conf/httpd.conf`. |
| `pgData` | string | PostgreSQL | Path `PGDATA`. |
| `pgAccount` | string | PostgreSQL | Service account, default `NT AUTHORITY\NetworkService`. |
| `stdoutLog` | string | Tidak | File log stdout (NSSM) / log aplikasi. |
| `stderrLog` | string | Tidak | File log stderr (NSSM). |
| `startup` | string | Tidak | `auto`, `delayed`, atau `manual`. |
| `restartOnCrash` | boolean | Tidak | Restart otomatis saat crash (NSSM), default `true`. |
| `restartDelaySec` | integer | Tidak | Jeda restart dalam detik (minimal 1), default `5`. |
| `dependencies` | array/string | Tidak | Nama service dependensi. String dipisahkan spasi/koma. |
| `addedAt` | string | Tidak | Waktu dibuat (ISO 8601). |
| `updatedAt` | string | Tidak | Waktu diperbarui (ISO 8601). |

## Jenis Service

### 1. NSSM (`nssm`)

Aplikasi biasa dibungkus NSSM. Service Manager memakai `nssm` untuk mengatur `Application`, `AppParameters`, `AppDirectory`, stdout/stderr, startup, restart, dan dependensi.

```json
{
  "name": "my-node-app",
  "displayName": "My Node App",
  "serviceType": "nssm",
  "executable": "C:\\Program Files\\nodejs\\node.exe",
  "arguments": "index.js",
  "workingDirectory": "C:\\apps\\my-node-app",
  "stdoutLog": "C:\\apps\\my-node-app\\logs\\out.log",
  "stderrLog": "C:\\apps\\my-node-app\\logs\\err.log",
  "startup": "auto",
  "restartOnCrash": true,
  "restartDelaySec": 5,
  "dependencies": ["Tcpip"]
}
```

Untuk file `.cmd`/`.bat`, gunakan `C:\Windows\System32\cmd.exe` sebagai `executable` dan isi `arguments` dengan `/c "C:\path\file.cmd"`.

### 2. Apache (`apache`)

Install:
```
httpd.exe -d <apacheRoot> -f <apacheConfig> -k install -n <name>
```
Uninstall:
```
httpd.exe -d <apacheRoot> -f <apacheConfig> -k uninstall -n <name>
```

```json
{
  "name": "Apache2.4",
  "displayName": "Apache 2.4",
  "serviceType": "apache",
  "executable": "C:\\Apache24\\bin\\httpd.exe",
  "apacheRoot": "C:\\Apache24",
  "apacheConfig": "conf/httpd.conf",
  "startup": "auto"
}
```

### 3. PostgreSQL (`postgresql`)

Install:
```
pg_ctl.exe register -N <name> -D <pgData> -U <pgAccount> -S auto -w
```
Uninstall:
```
pg_ctl.exe unregister -N <name>
```

```json
{
  "name": "postgresql-16",
  "displayName": "PostgreSQL 16",
  "serviceType": "postgresql",
  "executable": "C:\\Program Files\\PostgreSQL\\16\\bin\\pg_ctl.exe",
  "pgData": "C:\\Program Files\\PostgreSQL\\16\\data",
  "pgAccount": "NT AUTHORITY\\NetworkService",
  "startup": "auto"
}
```

## Urutan Dependensi

Saat **Start All**, service diurutkan secara topologi berdasarkan `dependencies`. Saat **Stop All**, urutan dibalik. Dependensi yang tidak ada di daftar aplikasi diabaikan untuk pengurutan. Batch berjalan di background sehingga UI tetap responsif.

## Backup dan Restore

- Setiap penyimpanan config membuat backup otomatis di `backup/` (maksimal 20 file terbaru).
- Tombol **Backup** menyimpan salinan JSON ke `backup/services-export-<timestamp>.json`.
- Tombol **Restore** memulihkan dari file JSON.
- Tombol **Import** memindai service Windows yang sudah terpasang (NSSM/Apache/PostgreSQL) dan menambahkannya ke config.

## Startup Mode

| Nilai | Arti |
| --- | --- |
| `auto` | Automatic |
| `delayed` | Automatic (Delayed Start) |
| `manual` | Manual |

Mode `delayed` diterapkan melalui Windows API `ChangeServiceConfig2` (`SERVICE_CONFIG_DELAYED_AUTO_START_INFO`).

## Settings Global (`settings.json`)

`data/settings.json` menyimpan path toolchain dan default updater. Contoh:
[settings.example.json](../settings.example.json). File runtime tidak di-commit
(masuk `.gitignore`).

```json
{
  "version": 1,
  "tools": {
    "git": { "path": "C:\\Program Files\\Git\\cmd\\git.exe", "version": "", "detectedAt": "" },
    "php": { "path": "C:\\path\\to\\php.exe", "version": "", "detectedAt": "" },
    "apache": {
      "path": "C:\\path\\to\\apache\\bin\\httpd.exe",
      "root": "C:\\path\\to\\apache",
      "config": "conf/httpd.conf",
      "version": "",
      "detectedAt": ""
    }
  },
  "updater": {
    "logDir": "C:\\tools\\ServiceManager\\logs",
    "maxConcurrentUpdates": 1,
    "timeouts": {
      "preflightSeconds": 30,
      "fetchSeconds": 120,
      "toolSeconds": 300,
      "cloneSeconds": 600
    },
    "logRetentionDays": 30
  },
  "trigger": { "allowedClientIPs": ["127.0.0.1", "::1"], "allowLocal": true },
  "privilegedExecution": {
    "mode": "scheduled-task",
    "taskName": "ServiceManager-Updater",
    "userId": "",
    "logonType": "Interactive"
  },
  "variables": {}
}
```

Catatan:

- Tool yang wajib diisi: `git`, `php`, `composer`, `npm`. `node`, `pnpm`, `yarn`,
  `bun`, `go`, `apache`, dan `nssm` opsional.
- `version` dan `detectedAt` diisi oleh deteksi tool.
- Password/token plaintext tidak diizinkan; password tetap memakai DPAPI di file
  terpisah.
- Jika file belum ada, `sm-updater` memakai `settings`/`trigger` lama di
  `projects.json` sebagai fallback tanpa menulis `settings.json`. File baru dibuat
  hanya oleh aksi simpan/migrasi eksplisit.
- Jika `settings.json` ada, `trigger.allowedClientIPs` dan `trigger.allowLocal` dari
  file itu **mengalahkan** `projects.json.trigger`. Per-app
  `apps[].allowedClientIPs` tetap menjadi override.
- `allowedClientIPs` menerima alamat IP dan CIDR IPv4/IPv6. Entri invalid atau kosong
  ditolak saat load/save. Daftar kosong berarti semua IP remote diizinkan; trigger
  lokal dikontrol `allowLocal`.
- Di GUI, tab **Settings** membaca nilai efektif (fallback legacy ditandai di
  Diagnostics) dan menulis `data/settings.json` hanya saat **Save Settings** atau
  **Migrate Trigger**.
- **Migrate Trigger** adalah aksi eksplisit: menyalin `projects.json.trigger` ke
  `settings.json.trigger`; `projects.json` tidak diubah.

## Project Registry (`projects.json` v2)

`data/projects.json` menyimpan app yang di-update. Versi 1 tetap dibaca dan dimigrasikan
di memori. Contoh lengkap: [projects.example.json](../projects.example.json) dan
[RUST-UPDATER.md](RUST-UPDATER.md).

```json
{
  "version": 2,
  "settings": {
    "gitPath": "C:\\Program Files\\Git\\cmd\\git.exe",
    "composerPath": "C:\\laragon\\bin\\composer\\composer.bat",
    "npmPath": "C:\\Program Files\\nodejs\\npm.cmd",
    "bunPath": "C:\\Users\\EGOV22\\.bun\\bin\\bun.exe",
    "goPath": "C:\\Program Files\\Go\\bin\\go.exe",
    "logDir": "C:\\tools\\ServiceManager\\logs"
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
      "preset": "laravel",
      "composer": "auto",
      "artisan": "auto",
      "packageManager": "npm",
      "packageInstall": "auto",
      "build": "auto",
      "queueRestart": true,
      "webServerService": "Apache2.4",
      "services": {
        "stopBeforePull": [],
        "stopBeforeBuild": [],
        "restartAfter": [],
        "startAfter": [],
        "waitTimeoutSeconds": 60
      },
      "healthCheck": { "enabled": false, "url": "", "timeoutSeconds": 60 },
      "notify": { "webhookUrl": "", "on": [] }
    }
  ]
}
```

Field penting:

| Field | Keterangan |
| --- | --- |
| `trigger.allowedClientIPs` | Allowlist global IP/CIDR; `settings.json` menang bila ada |
| `apps[].allowedClientIPs` | Override allowlist untuk satu app |
| `apps[].webServerService` | Nama service Apache opsional di `services.json`; divalidasi `serviceType=apache` |
| `preset` | `auto`, `laravel`, `node`, `bun`, `go`, `static`, `custom` |
| `packageManager` | `auto`, `npm`, `pnpm`, `yarn`, `bun`, `none` |
| `composer` / `artisan` / `packageInstall` / `build` | `auto`, `always`, `never` |
| `cloneOnFirstRun` | Clone repo jika `localPath` belum ada |
| `services.*` | Lifecycle stop/restart/start service Windows |
| `healthCheck` | HTTP check setelah deploy; gagal → `degraded` |
| `notify` | Webhook best-effort untuk status di `on` |

## Remote Trigger (`trigger-client.json`)

Dipakai tab **Remote** di `sm-gui`:

```json
{
  "host": "10.255.240.160",
  "user": "EGOV22",
  "port": 22,
  "identity": "",
  "authMode": "key",
  "rememberPassword": false,
  "passwordProtected": "",
  "wrapper": "C:\\tools\\ServiceManager\\scripts\\Invoke-UpdaterTask.ps1",
  "remoteBin": "C:\\tools\\ServiceManager\\target\\release\\sm-updater.exe",
  "remoteConfig": "",
  "app": ""
}
```

Field autentikasi:

| Field | Keterangan |
| --- | --- |
| `authMode` | `key` atau `password` |
| `identity` | Path private key (mode key; opsional jika memakai ssh-agent) |
| `rememberPassword` | Simpan password dengan DPAPI jika `true` |
| `passwordProtected` | Ciphertext DPAPI (hex); bukan password plaintext |
| `password` | Hanya ada di memori saat runtime, tidak ditulis ke file |

File ini hanya berisi alamat/user/path, bukan password plaintext. Mode password
memakai `SSH_ASKPASS`; mode key memakai `BatchMode=yes`. Detail:
[RUST-TRIGGER.md](RUST-TRIGGER.md).

## Service Global dan Per Project

Lihat [RUST-SERVICES.md](RUST-SERVICES.md) untuk pola Laravel scheduler global (Cronical) dan queue worker per project.
