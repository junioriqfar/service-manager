# Konfigurasi Service Manager

Dokumen ini menjelaskan format config runtime yang dipakai aplikasi Rust (`sm-gui`,
`sm-updater`, `sm-trigger`): service Windows, registry project, dan konfigurasi Remote
Trigger.

## Lokasi File

| Item | Path |
| --- | --- |
| Config service | `<folder config>\data\services.json` |
| Config project | `<folder config>\data\projects.json` |
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
| `trigger.allowedClientIPs` | Allowlist global IP/CIDR dari `SSH_CONNECTION`/`SSH_CLIENT` |
| `apps[].allowedClientIPs` | Override allowlist untuk satu app |
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
