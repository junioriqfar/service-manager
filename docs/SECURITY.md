# Keamanan

Dokumen ini menjelaskan praktik keamanan dan hal-hal yang **tidak boleh** dimasukkan ke repositori.

## Jangan Commit Kredensial

Jangan pernah menambahkan ke Git:

- Personal Access Token (PAT), OAuth token, atau password.
- Private key SSH (`id_rsa`, `id_ed25519`, `*.pem`, `*.key`).
- File `.env` yang berisi kredensial.
- Isi Windows Credential Manager atau hasil `git credential fill`.
- File konfigurasi runtime yang memuat informasi internal server.

`.gitignore` sudah mengecualikan:

```
services.json
projects.json
services-export-*.json
logs/
backup/
.env
*.token
*.secret
*.pfx
*.pem
*.key
id_rsa*
id_ed25519*
```

Gunakan `services.example.json` dan `projects.example.json` sebagai template.

## Penyimpanan Kredensial Git

`Store-GitCredential.ps1` menyimpan kredensial melalui Git Credential Manager:

- `GCM_CREDENTIAL_STORE=dpapi` - kredensial dienkripsi oleh Windows DPAPI untuk akun pengguna aktif.
- `GCM_INTERACTIVE=never` dan `GIT_TERMINAL_PROMPT=0` - mencegah prompt interaktif.
- Token dibaca sebagai `SecureString` dan memorinya dibersihkan setelah dipakai (`ZeroFreeBSTR`).
- Token **tidak** ditulis ke file script maupun repositori.

Jalankan helper ini **di server yang sama** dan **sebagai akun yang sama** dengan yang menjalankan updater (misalnya `EGOV22`). Kredensial DPAPI terikat pada user + machine, sehingga kredensial milik akun lain tidak akan terpakai.

Contoh:

```powershell
.\Store-GitCredential.ps1 -HostName gitlab.com -UserName oauth2 -ProjectPath D:\Projects\PHPProjects\contoh -GitPath C:\laragon\bin\git\cmd\git.exe
```

Gunakan PAT dengan hak minimum:

- Read-only untuk update: `read_repository`.
- Bila perlu push: `repo` (GitHub) / `write_repository` (GitLab).

Rotasi token secara berkala dan cabut token yang tidak lagi dipakai.

## Prinsip Hak Akses

- Aplikasi dan updater membutuhkan **Administrator** karena mengelola Windows Service.
- Batasi akses RDP/SSH ke server hanya untuk operator tepercaya.
- Jangan menjalankan service dengan akun domain berhak tinggi bila tidak perlu.
- Gunakan `NT AUTHORITY\NetworkService` atau service account khusus untuk PostgreSQL/aplikasi.

## Data Sensitif di Konfigurasi

`services.json` dan `projects.json` dapat memuat:

- path absolut server,
- nama server/akun,
- IP internal (`allowedClientIPs`),
- nama project dan branch.

Karena itu keduanya **tidak di-commit**. Bila perlu dibagikan, gunakan example file yang sudah dianonimkan.

## Audit Sebelum Commit

Sebelum commit, periksa:

```powershell
git status
git diff --cached
```

atau cari pola sensitif:

```powershell
git grep -n -i -E "password|token|secret|api[_-]?key|authorization|ghp_|gho_|glpat-"
```

Jika token sudah terlanjur ter-commit:

1. **Cabut/rotasi token** tersebut segera.
2. Hapus dari riwayat Git (misalnya `git filter-repo` atau BFG Repo-Cleaner).
3. Force-push hanya bila disepakati dan aman.
