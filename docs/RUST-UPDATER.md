# Project Updater Rust (`sm-updater`)

Implementasi Rust untuk `ProjectUpdate.ps1`. Binary mandiri tanpa PowerShell script, tetapi tetap kompatibel dengan `projects.json` versi 1.

## Build

```powershell
cd service-manager-rs
cargo build --release -p sm-updater
# hasil: target\release\sm-updater.exe
```

## Penggunaan

```powershell
sm-updater                          # menu interaktif
sm-updater list                     # daftar project
sm-updater check                    # cek semua project aktif (dry-run)
sm-updater check --project kasirin  # cek satu project
sm-updater update                   # update semua project aktif
sm-updater update --project kasirin # update satu project
sm-updater --config C:\path\projects.json
sm-updater --help
```

Exit code:

| Kode | Arti |
| --- | --- |
| `0` | Sukses / tidak ada kegagalan |
| `1` | Config tidak valid, project tidak ditemukan, atau ada update gagal |
| `2` | Argumen CLI tidak valid |

`--all` dapat dipakai eksplisit; tanpa `--project`, semua project dengan `enabled: true` diproses (sama seperti PowerShell).

## Alur Update

1. Validasi project Laravel (`.git`, `artisan`, `composer.json`).
2. Cek working tree bersih (`git status --porcelain`). Dirty → project dilewati.
3. Validasi branch aktif sama dengan konfigurasi.
4. Preflight `git ls-remote --heads origin` (timeout 30 detik).
5. `git fetch --prune origin` (timeout 120 detik).
6. Bandingkan `HEAD..origin/<branch>`.
7. Deteksi perubahan `database/migrations/*`:
   - mode `check` tetap melaporkan;
   - mode `update` **membatalkan** sebelum pull.
8. `git merge --ff-only origin/<branch>`.
9. Hitung langkah kondisional dari file yang berubah (`sm-core::plan_update`).
10. Jalankan composer/npm/build/artisan/queue sesuai kebutuhan.
11. Jalankan `postUpdateCommand` bila diisi.
12. Tulis log + rollback hint (`git reset --hard <sha-sebelum>`).

## Langkah Kondisional

| Perubahan | Aksi |
| --- | --- |
| `composer.json` / `composer.lock` | `composer install --no-dev --optimize-autoloader --no-interaction` |
| `package-lock.json` | `npm ci` lalu `npm run build` |
| `package.json` | `npm install` lalu `npm run build` |
| `resources/*`, `vite.config.*`, `tsconfig.json` | `npm run build` |
| Cache Laravel ada | `php artisan optimize:clear` |
| Kode berubah + `queueRestart` | `php artisan queue:restart` |
| `database/migrations/*` | Blokir update otomatis |

File `.bat`/`.cmd` (composer, npm) dijalankan melalui `cmd.exe /c`, sehingga tidak bergantung pada asosiasi shell.

## Status

`up-to-date`, `check`, `updated`, `dirty`, `blocked-migration`, `failed`.

## Log

- `logDir/project-update-YYYYMMDD-HHmmss.log`
- `<project>/storage/logs/project-update-YYYYMMDD-HHmmss.log`

Log berisi perintah yang dijalankan, output (maksimal 60 baris per perintah), commit/file berubah, dan rollback hint.

## Simpan Kredensial Git

Gunakan subcommand credential (padanan `Store-GitCredential.ps1`):

```powershell
sm-updater credential store `
  --host gitlab.com `
  --username oauth2 `
  --project D:\Projects\PHPProjects\open-po `
  --git C:\laragon\bin\git\cmd\git.exe
```

Token diminta tanpa echo, dikirim ke `git credential approve` melalui stdin, lalu diverifikasi dengan `git ls-remote --heads origin`. Token tidak pernah ditulis ke log/file. Default host `gitlab.com`, username `oauth2`, project = folder aktif; `--git` dicari otomatis dari `SERVICE_MANAGER_GIT`, Laragon, Program Files, atau `where git`.

## Keamanan

- Env otomatis: `GCM_CREDENTIAL_STORE=dpapi`, `GCM_INTERACTIVE=never`, `GIT_TERMINAL_PROMPT=0`.
- Preflight autentikasi sebelum menyentuh working tree.
- Tidak ada token/password di log atau config; log di-redaksi dengan `sm_core::redact_secrets`.
- DPAPI tersedia di `sm-win::dpapi` untuk kebutuhan enkripsi lokal lain.
- Perintah dijalankan dengan argumen array (tanpa shell string) kecuali `postUpdateCommand` yang memang PowerShell.
- Timeout eksplisit: preflight 30 detik, fetch/merge 120 detik, tool 300 detik.

## Pengujian

```powershell
cargo test -p sm-updater
cargo clippy -p sm-updater --all-targets -- -D warnings
```

27 unit test mencakup: parsing CLI, scan project, runner real (exit code/timeout), log, dan seluruh matriks update (up-to-date, check, dirty, branch mismatch, auth failure, migration block, composer failure, post-update command, langkah kondisional).

Verifikasi end-to-end dengan repository Git lokal:

- `list` mendeteksi project;
- `check` melaporkan `up-to-date` lalu `check` saat ada commit baru;
- `update` melakukan fast-forward dan mencatat rollback SHA;
- commit `database/migrations/*` menghasilkan `blocked-migration` tanpa pull.

## Task Scheduler

```
Program : C:\tools\ServiceManager\service-manager-rs\target\release\sm-updater.exe
Argument: update --config C:\tools\ServiceManager\projects.json
```

Jalankan sebagai akun yang memiliki kredensial Git tersimpan (mis. `EGOV22`) agar DPAPI dapat mengaksesnya.
