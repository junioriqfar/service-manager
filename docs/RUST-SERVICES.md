# Service Global dan Per Project

Dokumen ini menjelaskan pola penempatan service Laravel di Service Manager, khususnya scheduler dan queue.

## Ringkasan

| Kebutuhan | Pola yang dipakai | Jumlah Windows Service |
| --- | --- | --- |
| Laravel scheduler banyak project | Satu Cronical service, banyak job | 1 |
| Queue Laravel | Satu worker per project (Opsi A) | N project |
| Queue banyak project dalam satu service | Supervisor multi-worker (Opsi B, belum diimplementasikan) | 1 |

## 1. Scheduler Global dengan Cronical

Cronical adalah daemon scheduler yang membaca `cronical.dat`. Satu service dapat menjalankan job untuk beberapa project.

Contoh isi `C:\laragon\bin\cronical\cronical.dat`:

```text
* * * * * C:\laragon\bin\php\php-8.3.16-Win32-vs16-x64\php.exe D:\Projects\PHPProjects\kasirin\artisan schedule:run
  Home = D:\Projects\PHPProjects\kasirin

* * * * * C:\laragon\bin\php\php-8.3.16-Win32-vs16-x64\php.exe D:\Projects\PHPProjects\open-po\artisan schedule:run
  Home = D:\Projects\PHPProjects\open-po
```

Catatan:

- Service Windows tetap bernama `KasirinCron` (display: `Kasirin Cronical`) meskipun sekarang melayani lebih dari satu project.
- Cronical mendeteksi perubahan `cronical.dat` dan reload otomatis; log mencatat `Definition file change detected` dan `All jobs reloaded`.
- Backup file sebelum mengedit: `cronical.dat.bak-YYYYMMDD-HHMMSS`.
- Log Cronical berada di folder instalasi (`cronical-YYYY-MM.log`) dan log service di `storage\logs\cronical-service.log` project terkait.
- Untuk project baru, tambahkan satu job + `Home`; tidak perlu service baru.

Verifikasi:

```powershell
sc.exe query KasirinCron
Get-Content C:\laragon\bin\cronical\cronical.dat
Get-Content C:\laragon\bin\cronical\cronical-2026-11.log -Tail 20
```

## 2. Queue per Project (Opsi A)

`php artisan queue:work` memuat codebase, autoload, koneksi database, dan job class dari satu project. Karena itu worker harus dijalankan dari folder project masing-masing.

Contoh dua service:

| Service | Working directory | Arguments | Log |
| --- | --- | --- | --- |
| `KasirinQueue` | `D:\Projects\PHPProjects\kasirin` | `artisan queue:work --sleep=3 --tries=3 --timeout=90` | `storage\logs\queue.log` |
| `OpenPoQueue` | `D:\Projects\PHPProjects\open-po` | `artisan queue:work --sleep=3 --tries=3 --timeout=90` | `storage\logs\queue.log` |

Contoh entry `services.json`:

```json
{
  "name": "OpenPoQueue",
  "displayName": "Open-PO Queue Worker",
  "serviceType": "nssm",
  "executable": "C:\\laragon\\bin\\php\\php-8.3.16-Win32-vs16-x64\\php.exe",
  "arguments": "artisan queue:work --sleep=3 --tries=3 --timeout=90",
  "workingDirectory": "D:\\Projects\\PHPProjects\\open-po",
  "stdoutLog": "D:\\Projects\\PHPProjects\\open-po\\storage\\logs\\queue.log",
  "stderrLog": "D:\\Projects\\PHPProjects\\open-po\\storage\\logs\\queue.log",
  "startup": "auto",
  "restartOnCrash": true,
  "restartDelaySec": 5,
  "dependencies": ["postgresql-16", "Tcpip"]
}
```

Install service membutuhkan Administrator. Dari GUI:

1. Jalankan `sm-gui.exe` sebagai Administrator.
2. Pilih service → **Edit** → **Save** untuk menerapkan config ke NSSM.
3. Klik **Start**.

Jika service sudah terlanjur dibuat dengan parameter default, gunakan **Edit → Save** (bukan Install) untuk menerapkan `AppParameters`/`AppDirectory`.

Verifikasi:

```powershell
sc.exe query OpenPoQueue
C:\tools\nssm\nssm.exe get OpenPoQueue AppParameters
C:\tools\nssm\nssm.exe get OpenPoQueue AppDirectory
Get-Content D:\Projects\PHPProjects\open-po\storage\logs\queue.log -Tail 20
```

## 3. Supervisor Multi-Project (Opsi B, belum diimplementasikan)

Alternatif satu Windows service untuk banyak queue worker adalah supervisor yang menjalankan beberapa child process `queue:work` (satu per project), memantau exit code, dan me-restart worker. Ini membutuhkan komponen tambahan dan belum termasuk dalam implementasi saat ini.

Keuntungan: satu service. Konsekuensi: logging/restart per worker harus dikelola supervisor, dan satu worker tetap terikat pada satu project.

## Rekomendasi

- Scheduler: gunakan Cronical global (sudah diterapkan).
- Queue: gunakan Opsi A (satu service per project) untuk stabilitas dan isolasi.
- Jika jumlah project bertambah banyak, pertimbangkan Opsi B atau grouping di UI.

## Keamanan

- Jangan menaruh kredensial di `services.json`.
- Log queue dapat memuat data aplikasi; batasi akses folder `storage\logs`.
- Service berjalan sebagai `LocalSystem`; pertimbangkan service account khusus bila perlu.
- Semua perubahan service/config membutuhkan Administrator.
