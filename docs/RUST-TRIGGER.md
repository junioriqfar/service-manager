# Cross-Platform Trigger Client (`sm-trigger`)

`sm-trigger` adalah client SSH kecil untuk memicu updater di server Windows dari
Windows, macOS, atau Linux. Tidak ada polling/daemon: perintah dijalankan sekali,
lalu selesai.

## Build

```bash
cargo build --release -p sm-trigger
```

Hasil:

- Windows: `target\release\sm-trigger.exe`
- macOS/Linux: `target/release/sm-trigger`

Client hanya membutuhkan executable `ssh` (OpenSSH) di `PATH`. Tidak ada dependensi
runtime lain.

## Penggunaan

```bash
# Semua app
sm-trigger --host 10.255.240.160 --user EGOV22 --all

# Satu app
sm-trigger --host 10.255.240.160 --user EGOV22 --app opencode-quota

# Dry-run di server (tidak mengubah file/service)
sm-trigger --host 10.255.240.160 --user EGOV22 --all --check

# Cetak perintah SSH tanpa menjalankannya
sm-trigger --host 10.255.240.160 --user EGOV22 --all --dry-run
```

Opsi:

| Opsi | Default | Keterangan |
| --- | --- | --- |
| `--host HOST` | wajib | Host server Windows |
| `--user USER` | `USERNAME`/`USER` | User SSH |
| `--port PORT` | `22` | Port SSH |
| `--identity PATH` | — | Private key (`ssh -i`) |
| `--ssh-option VALUE` | — | Opsi `ssh -o`, boleh berulang |
| `--remote-bin PATH` | `C:\tools\ServiceManager\target\release\sm-updater.exe` | Binary updater di server |
| `--wrapper PATH` | — | Script wrapper elevated di server |
| `--config PATH` | — | `projects.json` di server |
| `--app ID` | — | Satu app |
| `--all` | default | Semua app aktif |
| `--check` | false | Dry-run |
| `--dry-run`, `--print` | false | Hanya cetak perintah |
| `--help` | — | Bantuan |

Contoh dengan key dan host key checking:

```bash
sm-trigger --host 10.255.240.160 --user EGOV22 --all `
  --identity ~/.ssh/id_ed25519 `
  --ssh-option StrictHostKeyChecking=yes
```

## Mode Langsung vs Wrapper Elevated

### Langsung

Tanpa `--wrapper`, perintah remote adalah:

```text
C:\tools\ServiceManager\target\release\sm-updater.exe run --all
```

Cocok jika service lifecycle tidak dipakai atau sesi SSH sudah memiliki hak
Administrator.

### Wrapper Scheduled Task (disarankan)

Service stop/start membutuhkan token elevated. Gunakan wrapper:

```bash
sm-trigger --host 10.255.240.160 --user EGOV22 --all `
  --wrapper "C:\tools\ServiceManager\scripts\Invoke-UpdaterTask.ps1"
```

Alurnya:

1. `sm-trigger` menjalankan SSH.
2. `Invoke-UpdaterTask.ps1` memanggil `sm-updater authorize` untuk memvalidasi
   `SSH_CONNECTION`/`SSH_CLIENT` terhadap allowlist global/per-app.
3. Wrapper menulis request trigger (nonce, app, check, client IP) ke
   `data/updater-request.json`.
4. Wrapper menjalankan Scheduled Task `ServiceManager-Updater`.
5. Task berjalan elevated dan menjalankan `Run-UpdaterTaskAction.ps1`.
6. Action script menjalankan `sm-updater run ... --client-ip IP`, menulis
   `data/updater-result.json`, dan wrapper menampilkan log + exit code.

Request kedaluwarsa setelah 15 menit. Task memakai `MultipleInstances IgnoreNew`;
lock updater tetap mencegah dua update bersamaan.

## Instalasi Scheduled Task

Jalankan **sebagai Administrator** di server:

```powershell
.\scripts\Install-UpdaterTask.ps1 -UserId "$env:USERDOMAIN\$env:USERNAME"
```

Default:

- Task name: `ServiceManager-Updater`
- Logon type: `Interactive` (harus ada sesi user aktif)
- Run level: **Highest**
- Action: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File ...\Run-UpdaterTaskAction.ps1`
- Tidak ada trigger otomatis; task hanya dijalankan manual.

Untuk akun yang tidak selalu login, gunakan `-LogonType Password` (akan meminta
password) atau `-LogonType S4U`. S4U tidak selalu bisa mengakses kredensial jaringan
Git; `Password`/`Interactive` lebih aman untuk Git remote.

Daftar/log task:

```powershell
Get-ScheduledTask -TaskName ServiceManager-Updater
Get-ScheduledTaskInfo -TaskName ServiceManager-Updater
```

## Uji

Dari client:

```bash
sm-trigger --host 10.255.240.160 --user EGOV22 --all --check --dry-run
sm-trigger --host 10.255.240.160 --user EGOV22 --all --check
```

Langsung di server, tanpa SSH:

```powershell
.\scripts\Invoke-UpdaterTask.ps1 -Check
```

## Exit Code

| Kode | Arti |
| --- | --- |
| `0` | Sukses |
| `1` | Updater gagal / SSH gagal / timeout task |
| `2` | Argumen `sm-trigger` tidak valid |
| `3` | IP ditolak allowlist |
| lainnya | Diteruskan dari `sm-updater` di server |

## Keamanan

- IP dicek di sesi SSH sebelum task elevated dijalankan.
- Allowlist global dan per-app tetap berlaku karena wrapper meneruskan client IP
  ke proses updater.
- Hanya user SSH tepercaya yang boleh memiliki akses tulis ke folder config;
  user tersebut memang sudah bisa menjalankan updater langsung.
- Firewall Windows tetap membatasi port SSH ke IP allowlist.
- Jangan menyimpan token di URL repo atau argumen command line.
- Tidak ada trigger otomatis/polling.

## Troubleshooting

| Gejala | Penyebab / solusi |
| --- | --- |
| `ssh: command not found` | Install OpenSSH client |
| `Permission denied (publickey)` | Cek `--user`, `--identity`, `ssh-agent`, dan `authorized_keys` |
| `Scheduled Task ... belum dibuat` | Jalankan `Install-UpdaterTask.ps1` sebagai Administrator |
| Task jalan tapi wrapper timeout | Cek `Get-ScheduledTaskInfo`, log `logs/updater-task-*.log`, dan lock `data/updater.lock` |
| `IP ... tidak diizinkan` | Tambahkan IP/CIDR ke `trigger.allowedClientIPs` atau per-app `allowedClientIPs` |
| Service gagal stop/start | Task belum `RunLevel Highest` atau user bukan Administrator |
| Git remote gagal di task | Gunakan `-LogonType Interactive`/`Password` agar DPAPI/GCM user tersedia |

## Catatan Rilis

`Build-Release.ps1` ikut membangun `sm-trigger`, menyalin script wrapper ke folder
`scripts/`, dan menyertakan dokumentasi ini di paket portable.
