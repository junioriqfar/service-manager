# Keamanan Implementasi Rust

Dokumen ini melengkapi [SECURITY.md](SECURITY.md) untuk komponen Rust (`sm-win`, `sm-gui`, `sm-updater`).

## Ringkasan Kontrol

| Kontrol | Implementasi | Lokasi |
| --- | --- | --- |
| Kredensial Git | Git Credential Manager + DPAPI (`GCM_CREDENTIAL_STORE=dpapi`) | `sm-updater/src/credential.rs`, `runner.rs` |
| Enkripsi data lokal | Windows DPAPI (`CryptProtectData`/`CryptUnprotectData`) | `sm-win/src/dpapi.rs` |
| Redaksi log | Pola token/Bearer/password/URL userinfo | `sm-core/src/security.rs` |
| Rotasi log aplikasi | `app.log` > 5 MB → `app.log.1` | `sm-gui/src/util.rs` |
| Elevasi | Cek token admin + `ShellExecuteW runas` | `sm-win/src/elevation.rs`, `sm-gui/src/commands.rs` |
| Argumen proses | Array argumen, tanpa penggabungan shell | `sm-win`, `sm-updater` |
| Validasi input | Nama service, path executable, native fields | `sm-core/src/validation.rs` |
| Config runtime | Di-ignore Git, backup otomatis | `.gitignore`, `sm-config` |

## Kredensial Git

`sm-updater credential store` adalah padanan `Store-GitCredential.ps1`:

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

DPAPI **bukan** pengganti Credential Manager untuk token Git. Gunakan DPAPI hanya bila aplikasi perlu menyimpan rahasia lokal sendiri; saat ini token Git tetap di GCM.

## Redaksi Log

`sm-core::redact_secrets` diterapkan pada:

- log aplikasi GUI (`app.log`);
- log updater (run log + `storage/logs`);
- pesan error credential helper.

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

## Elevasi Administrator

- GUI memeriksa token admin saat start dan menampilkan banner jika tidak elevated.
- Tombol **Jalankan sebagai Administrator** memanggil `ShellExecuteW` dengan verb `runas`, lalu keluar.
- Operasi config (simpan/edit) tetap bisa dilakukan tanpa admin; install/start/stop/restart membutuhkan admin.
- Updater tidak memerlukan admin; hanya akses Git dan file project.

## Argumen dan Path

- Proses dijalankan dengan array argumen; tidak ada `cmd.exe /c` kecuali untuk `.bat`/`.cmd` (composer/npm) dengan path eksplisit.
- Nama service divalidasi: huruf, angka, titik, garis bawah, tanda hubung (1-256 karakter).
- Executable dan native path divalidasi keberadaannya sebelum install/save.
- `postUpdateCommand` adalah satu-satunya perintah yang dijalankan sebagai string PowerShell, dan hanya berasal dari `projects.json` yang dikontrol admin.
- Log menampilkan perintah dengan quoting, tetapi nilai sensitif tetap di-redaksi.

## Audit Sebelum Commit

```powershell
git grep -n -i -E "gh[pousr]_|glpat-|AKIA|BEGIN .*PRIVATE KEY"
git status
git diff --cached
```

Aturan `.gitignore`:

- `services.json`, `projects.json`, `services-export-*.json`;
- `logs/`, `backup/`;
- `.env`, token/key/pem/pfx, private key;
- `service-manager-rs/target/`, `sm-gui/gen/`.

## Pengujian Keamanan

```powershell
cargo test -p sm-core security
cargo test -p sm-win dpapi
cargo test -p sm-updater credential
cargo test -p sm-gui util
```

Test mencakup redaksi pola token, rotasi log, round-trip DPAPI, validasi credential request, dan pemanggilan sink kredensial tanpa menyentuh Credential Manager asli.

## Keterbatasan

- Belum ada code signing installer (fase rilis).
- Belum ada audit log tamper-evident.
- Redaksi berbasis pola, bukan deteksi rahasia semantik; tetap hindari menulis rahasia ke log.
