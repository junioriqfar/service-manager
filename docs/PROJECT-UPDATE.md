# Project Update

`ProjectUpdate.ps1` memperbarui project Laravel dari Git dengan alur yang aman. Script dirancang untuk dijalankan di server (misalnya lewat SSH/Task Scheduler) tanpa interaksi browser.

## Cara Kerja

1. Memindai `scanRoots` untuk folder yang memenuhi syarat project Laravel:
   - memiliki folder `.git`
   - memiliki file `artisan`
   - memiliki file `composer.json`
2. Membaca konfigurasi per-project dari `projects.json`.
3. Melakukan preflight `git ls-remote --heads origin` sebelum mengubah file.
4. Menjalankan `git fetch --prune origin`.
5. Membandingkan `HEAD..origin/<branch>`.
6. Jika ada perubahan `database/migrations/*`, update otomatis **dibatalkan** (migration harus manual).
7. `git merge --ff-only origin/<branch>`.
8. Menjalankan composer/npm/build/artisan sesuai perubahan file.
9. Menulis log ke `logs/project-update-*.log` dan `storage/logs` project.

## Konfigurasi `projects.json`

```json
{
  "version": 1,
  "scanRoots": ["D:\\Projects\\PHPProjects"],
  "allowedClientIPs": ["127.0.0.1", "::1"],
  "gitPath": "C:\\Program Files\\Git\\cmd\\git.exe",
  "phpPath": "C:\\php\\php.exe",
  "composerPath": "C:\\ProgramData\\ComposerSetup\\bin\\composer.bat",
  "npmPath": "C:\\Program Files\\nodejs\\npm.cmd",
  "logDir": "C:\\tools\\ServiceManager\\logs",
  "projects": {
    "nama-project": {
      "enabled": true,
      "branch": "main",
      "composer": "auto",
      "build": "auto",
      "queueRestart": true,
      "postUpdateCommand": ""
    }
  }
}
```

### Field Global

| Field | Keterangan |
| --- | --- |
| `scanRoots` | Daftar folder yang dipindai untuk project Laravel. |
| `allowedClientIPs` | Daftar IP yang diizinkan (dipakai oleh integrasi/otomasi terkait). |
| `gitPath` | Path `git.exe`. |
| `phpPath` | Path `php.exe`. |
| `composerPath` | Path `composer.bat`/`composer.phar`. |
| `npmPath` | Path `npm.cmd`. |
| `logDir` | Folder log khusus `ProjectUpdate.ps1`. |
| `projects` | Override/pendaftaran project manual. |

### Field Project

| Field | Nilai | Keterangan |
| --- | --- | --- |
| `enabled` | boolean | Ikut diproses pada mode `-All`. |
| `branch` | string | Branch yang diharapkan. Bila tidak cocok, update ditolak. |
| `composer` | `auto`/`always`/`never` | `auto` hanya saat `composer.json`/`composer.lock` berubah. |
| `build` | `auto`/`always`/`never` | `auto` saat file frontend berubah (`resources/*`, `vite.config.*`, `package*.json`, `tsconfig.json`). |
| `queueRestart` | boolean | Jalankan `php artisan queue:restart` bila kode berubah. |
| `postUpdateCommand` | string | Perintah PowerShell tambahan setelah update berhasil. |

Project juga dapat didaftarkan manual dengan menambahkan `path` di salah satu entry `projects` (dipakai bila folder tidak berada langsung di `scanRoots`).

## Mode Penggunaan

```powershell
# Menu interaktif
.\ProjectUpdate.ps1

# Daftar project
.\ProjectUpdate.ps1 -Mode List

# Cek update (dry-run, tidak mengubah file)
.\ProjectUpdate.ps1 -Mode Check -Project nama-project

# Update satu project
.\ProjectUpdate.ps1 -Mode Auto -Project nama-project

# Update semua project aktif
.\ProjectUpdate.ps1 -Mode Auto -All
```

### Parameter

| Parameter | Keterangan |
| --- | --- |
| `-ConfigPath` | Path `projects.json` alternatif. Default di folder script. |
| `-Mode` | `Menu`, `List`, `Check`, atau `Auto`. Default `Menu`. |
| `-Project` | Nama project tertentu. |
| `-All` | Proses semua project `enabled`. |
| `-Menu` | Pintasan untuk `-Mode Menu`. |

## Perilaku Kondisional

| Perubahan terdeteksi | Aksi |
| --- | --- |
| `composer.json` / `composer.lock` | `composer install --no-dev --optimize-autoloader --no-interaction` (jika `composer=auto`) |
| `package-lock.json` | `npm ci` lalu `npm run build` |
| `package.json` | `npm install` lalu `npm run build` |
| `resources/*`, `vite.config.*`, `tsconfig.json` | `npm run build` (jika `build=auto`) |
| File cache Laravel ada | `php artisan optimize:clear` |
| Kode backend berubah | `php artisan queue:restart` (jika `queueRestart=true`) |
| `database/migrations/*` berubah | Update otomatis **dibatalkan** |

## Hasil Eksekusi

`Invoke-ProjectUpdate` mengembalikan salah satu status:

| Status | Arti |
| --- | --- |
| `up-to-date` | Tidak ada commit baru. |
| `check` | Pemeriksaan selesai tanpa perubahan file. |
| `updated` | Update dan langkah lanjutan berhasil. |
| `dirty` | Working tree tidak bersih; project dilewati. |
| `blocked-migration` | Ada migration baru; tidak ada pull. |
| `failed` | Terjadi error (lihat log). |

## Keamanan

- `GCM_INTERACTIVE=never` dan `GIT_TERMINAL_PROMPT=0` mencegah prompt yang menggantung saat berjalan tanpa terminal.
- Preflight mendeteksi kegagalan autentikasi sebelum menyentuh working tree.
- Kredensial Git disimpan lewat Credential Manager (DPAPI), bukan di repo.
- Rollback code: `git reset --hard <sha-sebelum-update>` (dicatat di log).

## Penjadwalan

Windows Task Scheduler (jalankan sebagai akun service, misalnya `EGOV22`):

```
Program : powershell.exe
Argument: -NoProfile -ExecutionPolicy Bypass -File "C:\tools\ServiceManager\ProjectUpdate.ps1" -Mode Auto -All
```

Pastikan kredensial Git sudah tersimpan untuk akun tersebut (lihat `Store-GitCredential.ps1`).
