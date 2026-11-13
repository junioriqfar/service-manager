# Konfigurasi Service Manager

Dokumen ini menjelaskan format `services.json` yang dipakai `ServiceManager.ps1`.

## Lokasi File

| Item | Path |
| --- | --- |
| Config utama | `<folder aplikasi>\services.json` |
| Backup otomatis | `<folder aplikasi>\backup\services-*.json` |
| Log aplikasi | `<folder aplikasi>\logs\app.log` |
| NSSM | `..\nssm\nssm.exe` atau `C:\tools\nssm\nssm.exe` |

Jika `services.json` tidak ada, aplikasi dimulai dengan daftar kosong. Jika JSON tidak valid, file lama dicadangkan ke `backup/services-broken-<timestamp>.json` dan aplikasi tetap berjalan dengan daftar kosong.

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
| `name` | string | Ya | Nama service Windows (unik). |
| `displayName` | string | Tidak | Nama tampilan; default sama dengan `name`. |
| `serviceType` | string | Tidak | `nssm` (default), `apache`, atau `postgresql`. |
| `executable` | string | Ya | Path executable. Untuk Apache harus `httpd.exe`; PostgreSQL harus `pg_ctl.exe`. |
| `arguments` | string | Tidak | Argumen baris perintah untuk NSSM. |
| `workingDirectory` | string | Tidak | Direktori kerja proses. |
| `apacheRoot` | string | Apache | Root instalasi Apache (`ServerRoot`). |
| `apacheConfig` | string | Apache | Path config; relatif terhadap `apacheRoot` atau absolut. Default `conf/httpd.conf`. |
| `pgData` | string | PostgreSQL | Path `PGDATA`. |
| `pgAccount` | string | PostgreSQL | Service account, default `NT AUTHORITY\NetworkService`. |
| `stdoutLog` | string | Tidak | File log stdout (NSSM). |
| `stderrLog` | string | Tidak | File log stderr (NSSM). |
| `startup` | string | Tidak | `auto` (default), `delayed`, atau `manual`. |
| `restartOnCrash` | boolean | Tidak | Restart otomatis saat crash (NSSM), default `true`. |
| `restartDelaySec` | integer | Tidak | Jeda restart dalam detik (minimal 1), default `5`. |
| `dependencies` | array/string | Tidak | Nama service dependensi. String dipisahkan spasi/koma. |
| `addedAt` | string | Tidak | Waktu dibuat (ISO 8601). |
| `updatedAt` | string | Tidak | Waktu diperbarui (ISO 8601). |

## Jenis Service

### 1. NSSM (`nssm`)

Aplikasi biasa dibungkus NSSM. Aplikasi memakai perintah `nssm` untuk mengatur `Application`, `AppParameters`, `AppDirectory`, stdout/stderr, startup, dan restart.

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

Saat **Start Semua**, service diurutkan secara topologi berdasarkan `dependencies`. Saat **Stop Semua**, urutan dibalik. Dependensi yang tidak ada di daftar aplikasi diabaikan untuk pengurutan.

## Backup dan Restore

- Setiap penyimpanan config membuat backup otomatis di `backup/` (maksimal 20 file terbaru).
- Menu **Aplikasi > Backup Config (Export)...** menyimpan salinan JSON.
- Menu **Aplikasi > Restore Config (Import)...** memulihkan dari file JSON.
- Menu **Service > Import Service...** memindai service Windows yang sudah terpasang (NSSM/Apache/PostgreSQL) dan menambahkannya ke config.

## Startup Mode

| Nilai | Arti |
| --- | --- |
| `auto` | Automatic |
| `delayed` | Automatic (Delayed Start) |
| `manual` | Manual |

Mode `delayed` diterapkan melalui Windows API `ChangeServiceConfig2` (`SERVICE_CONFIG_DELAYED_AUTO_START_INFO`).
