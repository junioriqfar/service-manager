# Keamanan Implementasi Rust

Dokumen ini menjelaskan kontrol keamanan komponen Rust (`sm-win`, `sm-gui`, `sm-updater`).

## Ringkasan Kontrol

| Kontrol | Implementasi | Lokasi |
| --- | --- | --- |
| Kredensial Git | Git Credential Manager + DPAPI (`GCM_CREDENTIAL_STORE=dpapi`) | `sm-updater/src/credential.rs`, `runner.rs` |
| Enkripsi data lokal | Windows DPAPI (`CryptProtectData`/`CryptUnprotectData`) | `sm-win/src/dpapi.rs` |
| Redaksi log | Pola token/Bearer/password/URL userinfo | `sm-core/src/security.rs` |
| Rotasi log aplikasi | `app.log` > 5 MB → `app.log.1` | `sm-gui/src/util.rs` |
| Elevasi | Cek token admin + `ShellExecuteW runas` | `sm-win/src/elevation.rs`, `sm-gui/src/commands.rs` |
| SSH trigger IP allowlist | Global + per-app IP/CIDR dari `SSH_CONNECTION` | `sm-updater/src/ip.rs`, `trigger.rs` |
| Elevated updater task | Scheduled Task `RunLevel Highest`, wrapper + nonce/expiry | `scripts/`, `sm-gui/src/remote.rs` |
| SSH client GUI | Key: `BatchMode=yes`; password: `SSH_ASKPASS` + DPAPI | `sm-gui/src/remote.rs`, `sm-trigger/src/main.rs` |
| Argumen proses | Array argumen, tanpa penggabungan shell | `sm-win`, `sm-updater` |
| Command admin-only | `commands.*`/`buildCommand`/`shell` hanya dari config admin | `sm-updater/src/toolchain.rs` |
| Validasi input | Nama service, path executable, native fields | `sm-core/src/validation.rs` |
| Config runtime | Di-ignore Git, backup otomatis | `.gitignore`, `sm-config` |

## Kredensial Git

`sm-updater credential store` adalah helper kredensial Git:

```powershell
sm-updater credential store `
  --host gitlab.com `
  --username oauth2 `
  --project D:\Projects\PHPProjects\open-po `
  --git C:\laragon\bin\git\cmd\git.exe
```

Alur:

1. Token dibaca tanpa echo (`rpassword`).
2. Input `protocol/host/username/password` dikirim ke `git credential approve` melalui **stdin**.
3. GCM menyimpan token dengan DPAPI; program tidak menulis token ke file/log.
4. Verifikasi `git ls-remote --heads origin`; error di-redaksi sebelum ditampilkan.

Token dengan hak minimum:

- `read_repository` untuk update.
- `write_repository`/`repo` hanya bila perlu push.

Kredensial DPAPI terikat pada user + machine. Jalankan helper sebagai akun yang sama dengan updater (mis. `EGOV22`).

## DPAPI

`sm-win::dpapi` menyediakan:

```rust
sm_win::dpapi_protect(b"secret", Some(b"entropy"))?;
sm_win::dpapi_unprotect(&ciphertext, Some(b"entropy"))?;
```

- Scope default: user + machine saat ini.
- Entropy opsional untuk mengikat ciphertext ke konteks aplikasi.
- Buffer output dibebaskan dengan `LocalFree`.
- Data kosong dikembalikan sebagai kosong (tidak memanggil API).
- Test: round-trip tanpa entropy, dengan entropy, entropy salah, dan payload kosong.
- Password SSH mode "Remember password" di GUI memakai entropy
  `ServiceManager-RemoteTrigger`; ciphertext disimpan sebagai hex di
  `data/trigger-client.json`.

DPAPI **bukan** pengganti Credential Manager untuk token Git. Gunakan DPAPI hanya bila aplikasi perlu menyimpan rahasia lokal sendiri; saat ini token Git tetap di GCM.

## Redaksi Log

`sm-core::redact_secrets` diterapkan pada:

- log aplikasi GUI (`app.log`);
- log updater (run log + `storage/logs`);
- pesan error credential helper;
- pesan history updater (`data/updater-history.jsonl`) sebelum ditulis, dengan cap
  4000 karakter agar satu baris tidak membengkak;
- seluruh `detail` dan `hint` laporan Doctor (`sm-gui/src/doctor.rs`) sebelum
  dikirim ke UI.

Pola yang di-redaksi:

- Token provider: `ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`, `glpat-`, `xoxb-`, `xoxp-`, `sk-`, `AKIA`, `ASIA`.
- `Authorization: Bearer <token>`.
- `password=`, `passwd=`, `token=`, `secret=`, `api_key=`, `apikey=`, `access_token=`.
- Userinfo URL: `https://user:password@host/...`.

Contoh:

```
token=ghp_abcdefghijklmnop12345  ->  token=[REDACTED]
https://oauth2:glpat-xxx@host   ->  https://oauth2:[REDACTED]@host
```

Log service aplikasi tidak di-redaksi karena merupakan output aplikasi pengguna; jangan menulis rahasia ke log service.

## Rotasi Log

`app.log` dirotasi saat melewati 5 MB:

- file lama → `logs/app.log.1` (overwrite);
- file baru dibuat;
- log updater sudah per-run (`project-update-<timestamp>.log`), tidak perlu rotasi.

Retensi log bersifat **manual-only** lewat **Clean Logs Now** di Settings:

- hanya `project-update-*.log` dan `updater-task-*.log` langsung di log dir, serta
  `project-update-*.log` di `<localPath>/storage/logs` app terdaftar;
- file yang dimodifikasi dalam 5 menit terakhir tidak pernah dihapus;
- `logRetentionDays = 0` menonaktifkan retensi dan command mengembalikan error;
- tidak ada prune otomatis saat update;
- `app.log`, `logs/apache-apply.log`, dan `data/updater-history.jsonl` tidak
  disentuh.

## Audit History dan Client IP

`data/updater-history.jsonl` adalah audit trail append-only per app:

- Setiap app yang diproses menulis satu entry, termasuk `forbidden`, check-only,
  `updated`, `failed`, dan `degraded`.
- `clientIp` menyimpan IP trigger hasil `--client-ip` atau `SSH_CONNECTION`/
  `SSH_CLIENT`; trigger lokal tanpa IP ditulis sebagai string kosong.
- `commit` adalah HEAD best-effort setelah update; `durationMs` adalah durasi update.
- `message` di-redaksi dan dibatasi 4000 karakter. Token/password tidak boleh masuk
  history.
- History **tidak** dipangkas oleh retensi log dan tidak dihapus otomatis. Batas
  retensi audit saat ini adalah kebijakan operator (file dapat diarsipkan manual).
- Kegagalan menulis history hanya dilaporkan ke stderr; update tetap berjalan dan
  outcome tidak berubah.
- Audit apply Apache tetap terpisah di `logs/apache-apply.log` dan tidak berubah.

## SSH Trigger dan Allowlist IP

- `sm-updater authorize` memvalidasi IP client dari `SSH_CONNECTION`/`SSH_CLIENT`
  terhadap allowlist global dan per-app (IP tunggal, CIDR, IPv6).
- Wrapper `Invoke-UpdaterTask.ps1` menjalankan `authorize` **sebelum** memicu
  Scheduled Task elevated.
- Client IP diteruskan ke proses updater lewat `run --client-ip` agar allowlist
  per-app tetap berlaku setelah elevasi.
- `sm-trigger` memakai executable `ssh` sistem; tidak menyimpan password.
- GUI Remote mendukung key/agent (`BatchMode=yes`) atau password:
  - Password dikirim ke `ssh` melalui `SSH_ASKPASS` (`SSH_ASKPASS_REQUIRE=force`);
    helper adalah `sm-trigger.exe` yang membaca environment sementara.
  - Password tidak pernah masuk command line atau log.
  - Jika **Remember password** aktif, password disimpan dengan DPAPI di
    `data/trigger-client.json` sebagai ciphertext; tanpa itu hanya ada di memori.
  - Mode password memakai `PreferredAuthentications=password,keyboard-interactive`
    dan `NumberOfPasswordPrompts=1`.
  - Host key baru memakai `StrictHostKeyChecking=accept-new` (TOFU).
- Scheduled Task `ServiceManager-Updater` berjalan `RunLevel Highest`.
- Request file berisi nonce + timestamp; action menolak request lebih dari 15 menit
  dan task memakai `MultipleInstances IgnoreNew`.
- Tidak ada port HTTP baru; akses hanya lewat SSH dan firewall yang sudah ada.

## Elevasi Administrator

- GUI memeriksa token admin saat start dan menampilkan banner jika tidak elevated.
- Tombol **Jalankan sebagai Administrator** memanggil `ShellExecuteW` dengan verb `runas`, lalu keluar.
- Operasi config (simpan/edit) tetap bisa dilakukan tanpa admin; install/start/stop/restart membutuhkan admin.
- Updater tanpa service lifecycle tidak memerlukan admin; app dengan service lifecycle
  memakai preflight elevated dan **membatalkan seluruh run sebelum Git** jika tidak elevated.
- Trigger SSH dengan service lifecycle harus lewat wrapper Scheduled Task elevated.

## Argumen dan Path

- Proses dijalankan dengan array argumen; tidak ada `cmd.exe /c` kecuali untuk `.bat`/`.cmd` (composer/npm) dengan path eksplisit.
- Nama service divalidasi: huruf, angka, titik, garis bawah, tanda hubung (1-256 karakter).
- Executable dan native path divalidasi keberadaannya sebelum install/save.
- `preUpdateCommand`/`postUpdateCommand` adalah string PowerShell dari config admin.
- `commands.*` (program + args) dan field `shell` hanya berasal dari config admin;
  tidak ada perintah yang dibaca dari repository.
- Toolchain hanya menjalankan program yang dikonfigurasi/ditemukan di PATH, bukan script dari repo.
- Log menampilkan perintah dengan quoting, tetapi nilai sensitif tetap di-redaksi.

## Audit Sebelum Commit

```powershell
git grep -n -i -E "gh[pousr]_|glpat-|AKIA|BEGIN .*PRIVATE KEY"
git status
git diff --cached
```

Aturan `.gitignore`:

- `data/services.json`, `data/projects.json`, `data/services-export-*.json`;
- `data/trigger-client.json`, `data/updater-state.json`, `data/updater.lock`,
  `data/updater-request.json`, `data/updater-result.json`;
- `logs/`, `backup/`;
- `.env`, token/key/pem/pfx, private key;
- `/target/`, `/crates/sm-gui/gen/`, `/crates/sm-gui/ui/node_modules/`, `/crates/sm-gui/ui/dist/`.

## Pengujian Keamanan

```powershell
cargo test -p sm-core security
cargo test -p sm-win dpapi
cargo test -p sm-updater credential
cargo test -p sm-updater ip
cargo test -p sm-gui util
cargo test -p sm-gui remote
```

Test mencakup redaksi pola token, rotasi log, round-trip DPAPI, validasi credential request,
allowlist IP/CIDR, konfigurasi/preview Remote Trigger, dan pemanggilan sink kredensial
tanpa menyentuh Credential Manager asli.

## Keterbatasan

- Belum ada code signing installer (fase rilis).
- Belum ada audit log tamper-evident.
- Redaksi berbasis pola, bukan deteksi rahasia semantik; tetap hindari menulis rahasia ke log.
