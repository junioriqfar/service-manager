#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod batch;
mod commands;
mod state;
mod updater;
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
            updater::list_updater_apps,
            updater::save_updater_app,
            updater::delete_updater_app,
            updater::test_updater_app,
            updater::run_updater,
            updater::read_updater_log,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Service Manager");
}

#[cfg(test)]
mod ui_tests {
    const INDEX_HTML: &str = include_str!("../ui/index.html");
    const APP: &str = include_str!("../ui/src/App.tsx");
    const SERVICE_TABLE: &str = include_str!("../ui/src/components/service-table.tsx");
    const SERVICE_FORM: &str = include_str!("../ui/src/components/service-form-dialog.tsx");
    const SERVICE_ACTIONS: &str = include_str!("../ui/src/components/service-action-dialogs.tsx");
    const LOG_PANEL: &str = include_str!("../ui/src/components/log-panel.tsx");
    const BATCH_DIALOG: &str = include_str!("../ui/src/components/batch-dialog.tsx");
    const IMPORT_DIALOG: &str = include_str!("../ui/src/components/import-dialog.tsx");
    const ABOUT_DIALOG: &str = include_str!("../ui/src/components/about-dialog.tsx");
    const ADMIN_BANNER: &str = include_str!("../ui/src/components/admin-banner.tsx");
    const THEME_TOGGLE: &str = include_str!("../ui/src/components/theme-toggle.tsx");
    const THEME_PROVIDER: &str = include_str!("../ui/src/components/theme-provider.tsx");
    const UPDATER_VIEW: &str = include_str!("../ui/src/components/updater-view.tsx");
    const UPDATER_FORM: &str = include_str!("../ui/src/components/updater-form-dialog.tsx");

    fn all_sources() -> String {
        format!(
            "{INDEX_HTML}\n{APP}\n{SERVICE_TABLE}\n{SERVICE_FORM}\n{SERVICE_ACTIONS}\n{LOG_PANEL}\n{BATCH_DIALOG}\n{IMPORT_DIALOG}\n{ABOUT_DIALOG}\n{ADMIN_BANNER}\n{THEME_TOGGLE}\n{THEME_PROVIDER}\n{UPDATER_VIEW}\n{UPDATER_FORM}"
        )
    }

    #[test]
    fn ui_is_english() {
        assert!(INDEX_HTML.contains("lang=\"en\""));
        let sources = all_sources();
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
            assert!(!sources.contains(word), "untranslated UI word: {word}");
        }
        for label in [
            "Service Manager",
            "Service List",
            "Add Service",
            "Edit Service",
            "Right-click a service for actions",
            "Service log: (none selected)",
            "Auto refresh",
            "About Service Manager",
            "Updater Apps",
            "Add Updater App",
            "Manual SSH/GUI trigger only. No background polling.",
        ] {
            assert!(sources.contains(label), "missing English label: {label}");
        }
        for emoji in ["▶", "⏹", "🔄", "➕", "🗑", "☀", "🌙"] {
            assert!(!sources.contains(emoji), "emoji used as UI icon: {emoji}");
        }
    }

    #[test]
    fn ui_has_browse_buttons_and_context_menu() {
        assert!(SERVICE_FORM.contains("pick_executable"));
        assert!(SERVICE_FORM.contains("pick_directory"));
        assert!(SERVICE_FORM.contains("Browse…"));
        assert!(SERVICE_TABLE.contains("ContextMenu"));
        assert!(SERVICE_TABLE.contains("onEdit(service)"));
        for action in ["install", "start", "stop", "restart", "log", "delete"] {
            assert!(
                SERVICE_TABLE.contains(&format!("onAction(\"{action}\"")),
                "missing context action: {action}"
            );
        }
        for label in [
            "Edit", "Install", "Start", "Stop", "Restart", "Open Log", "Delete",
        ] {
            assert!(
                SERVICE_TABLE.contains(label),
                "missing context menu label: {label}"
            );
        }
    }

    #[test]
    fn top_toolbar_only_has_global_actions() {
        for label in [
            "Start All",
            "Stop All",
            "Restart All",
            "Backup",
            "Restore",
            "Folder",
            "App Log",
            "About",
        ] {
            assert!(APP.contains(label), "missing global button: {label}");
        }
        for id in [
            "btn-edit",
            "btn-delete",
            "btn-install",
            "btn-start",
            "btn-stop",
            "btn-restart",
        ] {
            assert!(
                !APP.contains(id) && !SERVICE_TABLE.contains(id),
                "per-service button still in toolbar: {id}"
            );
        }
        for label in ["Add", "Import", "Refresh"] {
            assert!(
                SERVICE_TABLE.contains(label),
                "missing service list button: {label}"
            );
        }
        assert!(THEME_TOGGLE.contains("Sun"));
        assert!(THEME_TOGGLE.contains("Moon"));
        assert!(THEME_TOGGLE.contains("Monitor"));
        assert!(THEME_PROVIDER.contains("localStorage"));
        assert!(THEME_PROVIDER.contains("service-manager-theme"));
        assert!(THEME_PROVIDER.contains("classList.toggle(\"dark\""));
        for source in [APP, SERVICE_TABLE, SERVICE_FORM] {
            assert!(source.contains("lucide-react"));
        }
    }

    #[test]
    fn updater_ui_has_registry_actions() {
        for label in [
            "Updater Apps",
            "Add App",
            "Check All",
            "Update All",
            "Refresh",
            "Reload",
            "Status",
            "Toolchain",
            "Services",
        ] {
            assert!(
                UPDATER_VIEW.contains(label),
                "missing updater view label: {label}"
            );
        }
        for command in [
            "list_updater_apps",
            "run_updater",
            "test_updater_app",
            "delete_updater_app",
            "read_updater_log",
            "save_updater_app",
        ] {
            assert!(
                UPDATER_VIEW.contains(command) || UPDATER_FORM.contains(command),
                "missing updater command: {command}"
            );
        }
        for field in [
            "stopBeforePull",
            "stopBeforeBuild",
            "restartAfter",
            "startAfter",
            "waitTimeoutSeconds",
            "buildCommand",
            "installCommand",
            "allowedClientIPs",
            "cloneOnFirstRun",
            "healthCheck",
            "notify",
        ] {
            assert!(
                UPDATER_FORM.contains(field),
                "missing updater form field: {field}"
            );
        }
        assert!(UPDATER_FORM.contains("pick_directory"));
        assert!(UPDATER_FORM.contains("Browse…"));
        assert!(UPDATER_VIEW.contains("lucide-react"));
        assert!(UPDATER_FORM.contains("lucide-react"));
    }
}
