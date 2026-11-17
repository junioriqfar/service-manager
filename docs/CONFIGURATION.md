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
| Pending perubahan Apache | `<folder config>\data\apache-pending.json` (tidak di-commit) |
| State updater | `<folder config>\data\updater-state.json` |
| History updater | `<folder config>\data\updater-history.jsonl` (append-only JSONL) |
| Lock updater | `<folder config>\data\updater.lock` |
| Override folder | Environment variable `SERVICE_MANAGER_HOME` |
| Backup otomatis | `<folder config>\backup\services-*.json` |
| Backup apply mapping | `<folder config>\backup\projects-*.json` |
| Export backup | `<folder config>\backup\services-export-*.json` |
| Export settings | Path pilihan user `settings-export-<tanggal>.json` (tidak memuat rahasia) |
| Log aplikasi | `<folder config>\logs\app.log` |
| Audit apply Apache | `<folder config>\logs\apache-apply.log` |
| NSSM | `..\nssm\nssm.exe` atau `C:\tools\nssm\nssm.exe` |

Jika `data/services.json` tidak ada, aplikasi dimulai dengan daftar kosong. Jika JSON tidak valid, file lama dicadangkan ke `backup/services-broken-<timestamp>.json` dan aplikasi tetap berjalan dengan daftar kosong.

File runtime berikut tidak di-commit (lihat `.gitignore`): `data/updater-state.json`,
`data/updater-history.jsonl`, `data/updater.lock`, `data/updater-request.json`,
`data/updater-result.json`, `data/trigger-client.json`, dan
`data/apache-pending.json`.

### History updater (`updater-history.jsonl`)

`data/updater-history.jsonl` adalah file append-only berisi satu JSON object per
baris:

```json
{"timestamp":"2026-11-17T10:00:00+07:00","appId":"kasirin","status":"updated","message":"...","commit":"abc1234","durationMs":1234,"clientIp":"10.255.240.143"}
```

- Ditulis trigger registry untuk setiap app yang diproses (termasuk `forbidden`,
  `up-to-date`, check-only, `failed`, dan `degraded`).
- `message` di-redaksi dan dibatasi 4000 karakter; `clientIp` kosong untuk trigger
  lokal tanpa IP.
- GUI membaca 50 entry terbaru per app (maksimal 500) dengan urutan newest-first.
- History **tidak** dipangkas oleh retensi log dan tidak boleh dihapus otomatis;
  file ini adalah audit trail. Rotasi/cap ukuran belum tersedia.

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
  "maintenance": { "enabled": false, "message": "" },
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
- `maintenance.enabled = true` menjeda update non-check dan trigger SSH. Run
  `--check` dan dry-run plan tetap diizinkan. `maintenance.message` opsional,
  di-trim, maksimal 500 karakter, tanpa karakter kontrol, dan ditampilkan di
  Doctor serta banner tab Updater. Maintenance tidak mengubah allowlist IP,
  elevation, atau autentikasi.
- Export/load settings memakai file JSON terpisah; lihat bagian berikut.

### Maintenance (`maintenance`)

| Field | Tipe | Default | Keterangan |
| --- | --- | --- | --- |
| `enabled` | boolean | `false` | Jika `true`, update non-check dan trigger SSH ditolak sebelum app/Git dijalankan. |
| `message` | string | `""` | Alasan/deploy window opsional; di-trim, maksimal 500 karakter, tanpa karakter kontrol. |

Saat maintenance aktif, `sm-updater run` (tanpa `--check`), wrapper SSH, dan
tombol Run/Update All di GUI gagal dengan pesan yang menyertakan `message`.
`sm-updater run --check`, `plan`, Doctor, dan Preview Plan tetap berjalan.
Tidak ada state/history yang ditulis untuk run yang diblokir. Disable hanya dari
tab **Settings**; banner tab Updater tidak memiliki shortcut.

### Export dan Load Settings

Tab Settings menyediakan:

- **Export Settings…** — menulis settings efektif dari `settings.json` (atau
  fallback legacy) ke pretty JSON atomik di path pilihan user, default nama
  `settings-export-<tanggal>.json`. File hanya berisi settings: **tidak** memuat
  `projects.json`, `services.json`, log, history, password, atau token. Path wajib
  berekstensi `.json`. Pola `settings-export-*.json` di-ignore Git.
- **Load Settings File…** — membaca, menormalisasi, dan memvalidasi file `.json`,
  lalu menggantinya sebagai draft di form. Tidak ada file yang ditulis dan tidak ada
  service/Apache yang diubah. Draft ditandai **Unsaved changes** dan baru berlaku
  setelah **Save Settings**.

JSON invalid, settings yang gagal validasi (mis. required tool kosong atau
`maxConcurrentUpdates = 0`), message dengan karakter kontrol, dan path non-`.json`
ditolak dengan error yang jelas. File settings aktif tidak berubah selama proses
export/load.

### Variabel command (`${NAME}`)

`settings.json.variables` menyimpan variabel admin tambahan. Selain itu backend
membangun variabel built-in dari path tool efektif:

| Variabel | Sumber |
| --- | --- |
| `PHP` | `tools.php.path` |
| `PHP_HOME` | folder induk `tools.php.path` |
| `GIT` | `tools.git.path` |
| `COMPOSER` | `tools.composer.path` |
| `NPM` | `tools.npm.path` |
| `BUN` | `tools.bun.path` |
| `GO` | `tools.go.path` |
| `APACHE` | `tools.apache.path` |
| `APACHE_HOME` | `tools.apache.root` |
| `APACHE_CONFIG` | `tools.apache.config` |

Nama built-in menang atas variabel admin dengan nama yang sama. Ekspansi:

- `${NAME}` diganti rekursif maksimal 8 level;
- placeholder tidak dikenal dibiarkan apa adanya;
- environment variable Windows **tidak** diekspansi (tidak ada `%VAR%` atau `$env:VAR`);
- tidak ada pembacaan file project/repo.

Dipakai untuk `installCommand`, `buildCommand`, `preUpdateCommand`,
`postUpdateCommand`, serta `program`, `args[]`, `cwd`, `shell`, dan value `env` pada
`commands.install/build/preUpdate/postUpdate`. Built-in path/args updater (Git,
Composer, npm, Bun, Go, PHP artisan) tidak melalui ekspansi.

Jika `tools.php.path` berubah, Save Settings mengembalikan `phpPathChange` berisi
referensi path/root PHP lama pada command app. Pencocokan case-insensitive dan
slash-insensitive; field yang sudah memakai `${PHP}`/`${PHP_HOME}` dilewati.
`projects.json` tidak diubah sampai user memilih **Use ${PHP}** atau
**Use new path** di dialog GUI. Command `replace_php_references` membackup
`backup/projects-<timestamp>.json`, hanya mengubah app/field yang direferensikan,
lalu menyimpan config secara atomik.

### Perubahan Apache dan `services.json`

**Save Settings tidak langsung menulis `services.json`.** Jika
`tools.apache.path`, `root`, `config`, atau `version` berubah, GUI:

1. Membackup `settings.json` lama ke `backup/settings-<timestamp>.json` (jika ada).
2. Menyimpan `settings.json` baru.
3. Menulis preview `data/apache-pending.json` berisi path/root/config/version lama dan
   baru, daftar service terdampak, app terkait, serta timestamp.
4. Menampilkan panel **Apache change pending** di tab Settings.

Service Apache dianggap terdampak jika `serviceType=apache` dan salah satu benar:

- `executable` sama dengan path Apache lama;
- `executable` berada di bawah `root` lama;
- `apacheRoot` sama dengan `root` lama;
- path Apache lama kosong (konfigurasi pertama), yang memilih **semua** service Apache.

Preview propagasi perubahan Apache hanya memakai `apps[].webServerService` eksplisit.
Deteksi DocumentRoot berjalan terpisah sebagai saran dan tidak otomatis mengubah
`webServerService`.

**Apply Now** membutuhkan Administrator dan:

- menyimpan `services.json` baru; backup otomatis dibuat di
  `backup/services-<timestamp>.json` sebelum file ditimpa;
- memperbarui `executable`, `apacheRoot`, dan `apacheConfig` (jika nilai baru tidak
  kosong) hanya untuk service terdampak;
- memperbarui Apache service `binPath` menjadi
  `"httpd.exe" -d "<root>" -f "<config>" -k runservice`;
- stop service yang berjalan, apply SCM, lalu start kembali service yang tadi
  berjalan (timeout 60 detik);
- jika gagal, memulihkan `services.json` lama, mencoba memasang ulang config SCM
  lama, dan mencoba start service yang tadi berjalan. Pending tetap disimpan;
- jika sukses, menghapus `apache-pending.json` dan menulis audit ke
  `logs/apache-apply.log`.

Tanpa Administrator, settings dan pending tetap tersimpan, tetapi apply ditolak.
Mengosongkan path Apache tidak mengubah executable service; propagation memerlukan
path baru. File pending dan log audit tidak di-commit.

### Deteksi DocumentRoot dan saran `webServerService`

Kartu **Apache App Mapping** di tab Settings memakai command
`detect_apache_mappings` untuk membaca config Apache dan
`apply_apache_mappings` untuk menulis mapping setelah user memilih app.

Sumber config:

1. `services.json` service dengan `serviceType=apache`. `apacheRoot` dan
   `apacheConfig` service yang dipilih dipakai lebih dulu.
2. Jika `apacheRoot`/`apacheConfig` service kosong, fallback ke
   `settings.json.tools.apache.root`/`config`.
3. Config path relatif di-resolve terhadap `apacheRoot`; path absolut dipakai
   apa adanya. Path kosong berarti `conf/httpd.conf`.

Yang diproses parser (read-only, tanpa menjalankan `httpd.exe`):

- baris komentar `#` dan baris kosong diabaikan; quote `"..."`/`'...'` didukung;
- `Define NAME value` menyimpan variabel (nama variabel tidak case-sensitive);
- `${VAR}` di-expand rekursif dengan batas kedalaman; variabel tidak dikenal
  menghasilkan warning dan directive terkait dilewati;
- `ServerRoot` dipakai untuk me-resolve `DocumentRoot` relatif dan include relatif;
- `Include`/`IncludeOptional` relatif dicoba terhadap `ServerRoot`, root Apache,
  lalu folder file config saat ini; wildcard seperti `conf/sites-enabled/*.conf`
  memakai enumerasi direktori;
- include bersarang didukung, dengan batas kedalaman 12, maksimal 200 file,
  visited set untuk mencegah loop, dan batas ukuran file 4 MB;
- `Include` wajib yang hilang menjadi warning; `IncludeOptional` yang hilang
  diabaikan; variabel/quote/argumen rusak menghasilkan warning.

Pencocokan dengan `apps[].localPath` bersifat case-insensitive untuk path Windows:

| Hubungan | `matchKind` | Confidence |
| --- | --- | --- |
| Path sama | `exact` | high |
| DocumentRoot berada di bawah localPath (mis. `app/public`) | `descendant` | high |
| localPath berada di bawah DocumentRoot | `ancestor` | medium |

Satu app hanya menghasilkan satu saran; exact/descendant dipilih sebelum ancestor.
App yang sudah memiliki `webServerService` dikembalikan sebagai `alreadyMapped`
dan tidak disarankan untuk ditimpa. Deteksi tidak menulis file apa pun.

**Apply Mappings** memvalidasi service target (`serviceType=apache`) dan semua app
id, membackup `backup/projects-<timestamp>.json`, lalu menulis
`webServerService` hanya pada app yang dipilih. `projects.json` disimpan atomik;
field app lain tidak diubah. Tidak ada apply otomatis dari hasil deteksi.

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
