#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apache;
mod apache_config;
mod batch;
mod commands;
mod remote;
mod settings;
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
            remote::load_remote_trigger,
            remote::save_remote_trigger,
            remote::preview_remote_trigger,
            remote::test_remote_ssh,
            remote::run_remote_trigger,
            settings::load_settings_view,
            settings::save_settings_view,
            settings::migrate_trigger_to_settings,
            apache::apache_change_status,
            apache::apply_apache_change,
            apache_config::detect_apache_mappings,
            apache_config::apply_apache_mappings,
            settings::detect_tool,
            settings::test_tool,
            settings::task_status,
            settings::install_task,
            settings::remove_task,
            settings::test_task,
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
    const REMOTE_VIEW: &str = include_str!("../ui/src/components/remote-trigger-view.tsx");
    const SETTINGS_VIEW: &str = include_str!("../ui/src/components/settings-view.tsx");
    const HINT: &str = include_str!("../ui/src/components/hint.tsx");
    const TOOLTIP: &str = include_str!("../ui/src/components/ui/tooltip.tsx");

    fn all_sources() -> String {
        format!(
            "{INDEX_HTML}\n{APP}\n{SERVICE_TABLE}\n{SERVICE_FORM}\n{SERVICE_ACTIONS}\n{LOG_PANEL}\n{BATCH_DIALOG}\n{IMPORT_DIALOG}\n{ABOUT_DIALOG}\n{ADMIN_BANNER}\n{THEME_TOGGLE}\n{THEME_PROVIDER}\n{UPDATER_VIEW}\n{UPDATER_FORM}\n{REMOTE_VIEW}\n{SETTINGS_VIEW}\n{HINT}\n{TOOLTIP}"
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
            "Remote Trigger",
            "Remote output",
            "Test SSH",
            "Check App",
            "Run App",
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
            "webServerService",
            "cloneOnFirstRun",
            "healthCheck",
            "notify",
        ] {
            assert!(
                UPDATER_FORM.contains(field),
                "missing updater form field: {field}"
            );
        }
        for label in ["Web server (Apache)", "Not linked", "PHP"] {
            assert!(
                UPDATER_FORM.contains(label) || UPDATER_VIEW.contains(label),
                "missing updater runtime label: {label}"
            );
        }
        assert!(UPDATER_FORM.contains("list_services"));
        assert!(UPDATER_FORM.contains("serviceType === \"apache\""));
        assert!(UPDATER_FORM.contains("pick_directory"));
        assert!(UPDATER_FORM.contains("Browse…"));
        assert!(UPDATER_VIEW.contains("lucide-react"));
        assert!(UPDATER_FORM.contains("lucide-react"));
        assert!(UPDATER_VIEW.contains("ContextMenu"));
        for action in ["Run", "Check", "Test", "Log", "Edit", "Delete"] {
            assert!(
                UPDATER_VIEW.contains(action),
                "missing updater context action: {action}"
            );
        }
    }

    #[test]
    fn remote_ui_has_trigger_actions() {
        for label in [
            "Remote Trigger",
            "Remote output",
            "Preview",
            "Test SSH",
            "Check All",
            "Update All",
            "Check App",
            "Run App",
            "Save",
            "Password",
            "Remember password",
        ] {
            assert!(REMOTE_VIEW.contains(label), "missing remote label: {label}");
        }
        for command in [
            "load_remote_trigger",
            "save_remote_trigger",
            "preview_remote_trigger",
            "test_remote_ssh",
            "run_remote_trigger",
            "remote-trigger-output",
        ] {
            assert!(
                REMOTE_VIEW.contains(command),
                "missing remote command/event: {command}"
            );
        }
        for field in [
            "remoteBin",
            "remoteConfig",
            "identity",
            "wrapper",
            "authMode",
            "DPAPI",
        ] {
            assert!(REMOTE_VIEW.contains(field), "missing remote field: {field}");
        }
        assert!(REMOTE_VIEW.contains("lucide-react"));
        assert!(APP.contains("TooltipProvider"));
        assert!(APP.contains("Hint"));
        assert!(HINT.contains("TooltipTrigger"));
        assert!(TOOLTIP.contains("TooltipPrimitive"));
    }

    #[test]
    fn settings_ui_has_toolchain_and_privileged_task_actions() {
        for label in [
            "Settings",
            "Toolchain",
            "Updater defaults",
            "Privileged Execution",
            "Diagnostics",
            "Git",
            "PHP",
            "Composer",
            "Node.js",
            "npm",
            "pnpm",
            "Yarn",
            "Bun",
            "Go",
            "Apache",
            "NSSM",
            "Auto-detect",
            "Browse…",
            "Test",
            "Save Settings",
            "Reload",
            "Install Task",
            "Repair Task",
            "Remove Task",
            "Test Task",
            "Legacy fallback",
            "Missing required tools",
            "Invalid tool path",
            "Scheduled Task not installed",
            "Trigger",
            "Allowed IPs / CIDR",
            "Allow local trigger",
            "Current client IP",
            "Migrate Trigger",
            "Apache change pending",
            "Affected services",
            "Affected apps",
            "Apply Now",
            "Later",
            "Review",
            "Audit log",
            "Administrator required",
            "Apache App Mapping",
            "Detect Apps",
            "Apply Mappings",
            "Already mapped",
            "Suggestions",
            "Parser warnings",
        ] {
            assert!(
                SETTINGS_VIEW.contains(label),
                "missing settings view label: {label}"
            );
        }
        for command in [
            "load_settings_view",
            "save_settings_view",
            "migrate_trigger_to_settings",
            "detect_tool",
            "test_tool",
            "task_status",
            "install_task",
            "remove_task",
            "test_task",
            "apache_change_status",
            "apply_apache_change",
            "detect_apache_mappings",
            "apply_apache_mappings",
            "list_services",
            "pick_executable",
            "pick_directory",
        ] {
            assert!(
                SETTINGS_VIEW.contains(command),
                "missing settings command: {command}"
            );
        }
        for field in [
            "logDir",
            "maxConcurrentUpdates",
            "preflightSeconds",
            "fetchSeconds",
            "toolSeconds",
            "cloneSeconds",
            "logRetentionDays",
            "taskName",
            "userId",
            "logonType",
            "allowedClientIPs",
            "allowLocal",
            "currentClientIp",
            "oldPath",
            "newPath",
            "oldRoot",
            "newRoot",
            "oldExecutable",
            "newExecutable",
            "webServerService",
            "documentRoot",
            "matchKind",
            "confidence",
            "existingService",
            "alreadyMapped",
        ] {
            assert!(
                SETTINGS_VIEW.contains(field),
                "missing settings field: {field}"
            );
        }
        assert!(SETTINGS_VIEW.contains("lucide-react"));
        assert!(APP.contains("SettingsView"));
        assert!(APP.contains("setView(\"settings\")"));
        assert!(APP.contains("view === \"settings\""));
        assert!(APP.contains("admin={admin}"));
        assert!(APP.contains("lucide-react"));
    }
}
