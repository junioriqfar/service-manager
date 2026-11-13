# Service Manager

Aplikasi GUI PowerShell untuk mengelola Windows Service di server Windows, khususnya service yang dijalankan lewat **NSSM**, serta service native **Apache (httpd)** dan **PostgreSQL (pg_ctl)**. Proyek ini juga menyertakan **Project Update** untuk memperbarui project Laravel dari Git secara aman.

> Dijalankan sebagai **Administrator** pada Windows dengan PowerShell 5.1.

## Fitur

### Service Manager (`ServiceManager.ps1`)

- Daftar service dalam tabel: status, nama, display name, jenis, executable, startup, dan restart.
- Tambah, edit, hapus, install, start, stop, dan restart service dari satu jendela.
- Dukungan tiga jenis service:
  - `nssm` - proses aplikasi apa pun yang dibungkus NSSM.
  - `apache` - service native Apache melalui `httpd.exe -k install`.
  - `postgresql` - service native PostgreSQL melalui `pg_ctl register`.
- Import otomatis service yang sudah terpasang di Windows (NSSM, Apache, PostgreSQL).
- Start/Stop/Restart semua service dengan urutan dependensi (topological sort).
- Startup mode: `auto`, `delayed` (Automatic Delayed Start), dan `manual`.
- Restart otomatis saat crash dengan jeda yang dapat diatur (khusus NSSM).
- Log viewer bawaan (auto refresh 2 detik) dan tombol buka file log.
- Backup otomatis `services.json` sebelum perubahan (maksimal 20 file) serta backup/restore config ke JSON.
- Deteksi NSSM di `..\nssm\nssm.exe` atau fallback `C:\tools\nssm\nssm.exe`.

### Project Update (`ProjectUpdate.ps1`)

- Memindai folder project Laravel di `scanRoots`.
- Mode `Menu`, `List`, `Check` (dry-run), dan `Auto` (pull + composer + build).
- Preflight akses remote sebelum mengubah file.
- Menolak update otomatis bila ada perubahan `database/migrations/*` (harus manual).
- Hanya melanjutkan bila working tree bersih dan branch sesuai konfigurasi.
- `composer install`, `npm ci`/`npm install`, `npm run build`, `php artisan optimize:clear`, dan `queue:restart` dijalankan secara kondisional.
- Log per-project disimpan di `storage/logs` project dan folder `logs` aplikasi.
- Mendukung `postUpdateCommand` dan rollback code (`git reset --hard <sha>`).

## Kebutuhan

| Komponen | Keterangan |
| --- | --- |
| Windows | Windows 10/11 atau Windows Server |
| PowerShell | Windows PowerShell 5.1 (bawaan Windows) |
| Hak akses | Administrator (aplikasi akan mencoba elevasi otomatis) |
| NSSM | Untuk service bertipe `nssm` (opsional bila hanya memakai service native) |
| Git | Untuk fitur Project Update dan penyimpanan kredensial |
| PHP / Composer / npm | Untuk Project Update project Laravel (path dapat dikonfigurasi) |

## Struktur Folder

```
ServiceManager/
├─ ServiceManager.ps1        # Aplikasi GUI utama
├─ ServiceManager.cmd        # Launcher (menjalankan PowerShell tersembunyi)
├─ ProjectUpdate.ps1         # Updater project Laravel
├─ Store-GitCredential.ps1   # Helper penyimpanan kredensial Git (DPAPI)
├─ services.example.json     # Contoh konfigurasi service
├─ projects.example.json     # Contoh konfigurasi project updater
├─ service-manager-rs/       # Implementasi Rust (bertahap)
├─ services.json             # Konfigurasi runtime (tidak di-commit)
├─ projects.json             # Konfigurasi runtime (tidak di-commit)
├─ backup/                   # Backup config otomatis (tidak di-commit)
└─ logs/                     # Log aplikasi & updater (tidak di-commit)
```

## Mulai Cepat

1. Clone repositori:
   ```powershell
   git clone https://github.com/junioriqfar/service-manager.git C:\tools\ServiceManager
   ```
2. Salin file contoh konfigurasi:
   ```powershell
   Copy-Item .\services.example.json .\services.json
   Copy-Item .\projects.example.json .\projects.json
   ```
3. Sesuaikan path di `services.json` dan `projects.json` dengan server Anda.
4. Jalankan `ServiceManager.cmd` (klik dua kali) atau:
   ```powershell
   powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\ServiceManager.ps1
   ```
5. Mode diagnosa (tanpa membuka GUI):
   ```powershell
   powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\ServiceManager.ps1 -Diagnose
   ```

## Konfigurasi

Detail skema konfigurasi, tipe service, dan contoh lengkap tersedia di [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

Ringkasan `services.json`:

```json
{
  "version": 1,
  "services": [
    {
      "name": "my-app",
      "displayName": "My App",
      "serviceType": "nssm",
      "executable": "C:\\Program Files\\nodejs\\node.exe",
      "arguments": "index.js",
      "workingDirectory": "C:\\apps\\my-app",
      "startup": "auto",
      "restartOnCrash": true,
      "restartDelaySec": 5,
      "dependencies": ["Tcpip"]
    }
  ]
}
```

## Project Update

Panduan lengkap ada di [docs/PROJECT-UPDATE.md](docs/PROJECT-UPDATE.md).

```powershell
# Menu interaktif
.\ProjectUpdate.ps1

# Daftar project yang terdeteksi
.\ProjectUpdate.ps1 -Mode List

# Cek update tanpa mengubah file
.\ProjectUpdate.ps1 -Mode Check -Project kasirin

# Update semua project aktif (pull + composer + build)
.\ProjectUpdate.ps1 -Mode Auto -All
```

## Keamanan

- **Jangan pernah commit token, password, PAT, atau private key.**
- `services.json`, `projects.json`, `logs/`, dan `backup/` dikecualikan melalui `.gitignore` karena berisi path/informasi spesifik server.
- Kredensial Git disimpan di Windows Credential Manager melalui Git Credential Manager dengan `GCM_CREDENTIAL_STORE=dpapi`, **bukan** di dalam repositori.
- Gunakan Personal Access Token (PAT) dengan scope minimal (`read_repository` atau `repo` bila perlu push) dan rotasi secara berkala.
- `Store-GitCredential.ps1` membaca token sebagai `SecureString` dan meneruskannya ke `git credential approve` tanpa menulis token ke disk/script.

Lihat [docs/SECURITY.md](docs/SECURITY.md) untuk detail.

## Troubleshooting

| Masalah | Solusi |
| --- | --- |
| Aplikasi tidak muncul / minta Administrator | Jalankan `ServiceManager.cmd`, atau buka PowerShell sebagai Administrator lalu jalankan script. |
| NSSM tidak ditemukan | Letakkan `nssm.exe` di `C:\tools\nssm\nssm.exe` atau satu level di atas folder aplikasi (`..\nssm\nssm.exe`). Service native tetap bisa dikelola. |
| `Unexpected status SERVICE_START_PENDING` | Proses service lambat start. Cek log service, naikkan waktu tunggu, dan pastikan executable/argumen benar. |
| Config rusak | File lama otomatis dicadangkan ke `backup/services-broken-*.json`. Perbaiki JSON lalu jalankan ulang. |
| `Akses remote gagal` pada Project Update | Simpan kredensial Git lewat `Store-GitCredential.ps1` atau login sekali via RDP/console. Browser tidak bisa muncul via SSH. |
| Update diblokir karena migration | Jalankan migration secara manual, lalu ulangi update. |

## Implementasi Rust

Versi Rust dikembangkan bertahap di folder [`service-manager-rs/`](service-manager-rs/README.md). Status saat ini: **Phase 0-3** — workspace, model domain, kompatibilitas `services.json`/`projects.json`, backup, SCM core, wrapper NSSM/Apache/PostgreSQL, import discovery, GUI Tauri, batch async anti-freeze, dan unit test.

```powershell
cd service-manager-rs
cargo run -p sm-gui
```

Panduan GUI: [docs/RUST-GUI.md](docs/RUST-GUI.md).

```powershell
cd service-manager-rs
cargo test --workspace
```

## Roadmap

Rencana migrasi ke Rust (arsitektur, pemetaan fitur, fase, estimasi, dan risiko) tersedia di [docs/RUST-REWRITE-PLAN.md](docs/RUST-REWRITE-PLAN.md). Status implementasi diperbarui di [service-manager-rs/README.md](service-manager-rs/README.md).

## Lisensi

Belum ditentukan.
