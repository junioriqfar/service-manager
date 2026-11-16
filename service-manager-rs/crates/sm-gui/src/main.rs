#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod batch;
mod commands;
mod state;
mod util;

fn main() {
    let paths = sm_config::ConfigPaths::detect();
    let _ = std::fs::create_dir_all(paths.logs_dir());

    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();

    tauri::Builder::default()
        .manage(state::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::list_services,
            commands::get_warning,
            commands::get_paths,
            commands::is_admin,
            commands::elevate,
            commands::read_log,
            commands::open_log,
            commands::open_app_folder,
            commands::open_app_log,
            commands::open_path,
            commands::pick_executable,
            commands::pick_directory,
            commands::save_service,
            commands::delete_service,
            commands::install_service,
            commands::start_service,
            commands::stop_service,
            commands::restart_service,
            commands::run_batch_action,
            commands::cancel_batch,
            commands::import_candidates,
            commands::import_services,
            commands::export_config,
            commands::restore_config,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Service Manager");
}

#[cfg(test)]
mod ui_tests {
    const HTML: &str = include_str!("../ui/index.html");
    const JS: &str = include_str!("../ui/app.js");

    #[test]
    fn ui_is_english() {
        assert!(HTML.contains("lang=\"en\""));
        for word in [
            "Tambah",
            "Hapus",
            "Simpan",
            "Batal",
            "Muat ulang",
            "Daftar",
            "Jenis",
            "terdaftar",
            "berjalan",
            "Tentang",
            "Tidak",
        ] {
            assert!(!HTML.contains(word), "untranslated HTML word: {word}");
        }
        for word in ["Simpan", "Batal", "Daftar", "Jenis", "berjalan", "Tidak"] {
            assert!(!JS.contains(word), "untranslated JS word: {word}");
        }
    }

    #[test]
    fn ui_has_browse_buttons_and_context_menu() {
        assert!(HTML.contains("id=\"btn-browse-exe\""));
        assert!(HTML.contains("id=\"btn-browse-work\""));
        assert!(HTML.contains("id=\"service-context-menu\""));
        for action in [
            "edit", "install", "start", "stop", "restart", "log", "delete",
        ] {
            assert!(
                HTML.contains(&format!("data-action=\"{action}\"")),
                "missing context action: {action}"
            );
        }
    }
}
