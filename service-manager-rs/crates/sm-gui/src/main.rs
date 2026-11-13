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
        .expect("gagal menjalankan Service Manager");
}
