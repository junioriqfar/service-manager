# Service Manager RS

Implementasi bertahap Service Manager dalam Rust. Status saat ini: **Phase 0 selesai** (workspace, domain model, kompatibilitas config, test).

Rencana lengkap: [../docs/RUST-REWRITE-PLAN.md](../docs/RUST-REWRITE-PLAN.md).

## Struktur

```
service-manager-rs/
├─ Cargo.toml
└─ crates/
   ├─ sm-core/     # model, validasi, ordering, aturan update
   └─ sm-config/   # services.json & projects.json, backup, atomic write
```

## Build dan Test

```powershell
cd service-manager-rs
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Saat ini semua test berjalan tanpa membutuhkan Administrator. Test yang menyentuh Windows Service akan ditandai `#[ignore]` dan dijalankan manual di server uji.

## Kompatibilitas Config

- `services.json` versi 1: object `{ version, services: [...] }` atau array legacy.
- `projects.json` versi 1.
- Field tidak dikenal diabaikan; nilai enum tidak dikenal dinormalisasi (tipe → NSSM, startup → Auto).
- Config rusak dicadangkan ke `backup/services-broken-<timestamp>.json`.
- Backup otomatis maksimal 20 file, pola `services-<timestamp>.json`.

## Environment

| Variabel | Fungsi |
| --- | --- |
| `SERVICE_MANAGER_HOME` | Folder config/log/backup. Default: folder executable. |

## Status Fase

| Fase | Status |
| --- | --- |
| 0. Fondasi | Selesai |
| 1. Core SCM | Belum |
| 2. NSSM & native | Belum |
| 3. GUI | Belum |
| 4. Updater | Belum |
| 5. Security & hardening | Belum |
| 6. Release | Belum |
