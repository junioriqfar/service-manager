#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apache;
mod apache_config;
mod batch;
mod commands;
mod credentials;
mod doctor;
mod firewall;
mod php_paths;
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
            updater::plan_updater_app,
            updater::load_updater_history,
            updater::run_updater,
            updater::read_updater_log,
            remote::load_remote_trigger,
            remote::save_remote_trigger,
            remote::preview_remote_trigger,
            remote::test_remote_ssh,
            remote::run_remote_trigger,
            settings::load_settings_view,
            settings::save_settings_view,
            settings::load_maintenance_status,
            settings::export_settings_file,
            settings::load_settings_file,
            settings::migrate_trigger_to_settings,
            settings::prune_logs_now,
            apache::apache_change_status,
            apache::apply_apache_change,
            apache_config::detect_apache_mappings,
            apache_config::apply_apache_mappings,
            php_paths::replace_php_references,
            settings::detect_tool,
            settings::test_tool,
            settings::task_status,
            settings::install_task,
            settings::remove_task,
            settings::test_task,
            doctor::run_doctor,
            credentials::list_git_credential_hosts,
            credentials::store_git_credential,
            credentials::test_git_credential,
            credentials::clear_git_credential,
            // UI command name: `ssh_firewall_status` (renamed in firewall.rs).
            firewall::read_ssh_firewall_status,
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
    const TYPES: &str = include_str!("../ui/src/lib/types.ts");
    const HINT: &str = include_str!("../ui/src/components/hint.tsx");
    const TOOLTIP: &str = include_str!("../ui/src/components/ui/tooltip.tsx");
    // Runtime process-spawning sources. Keep this list in sync when adding a
    // new GUI-side Command::new. Console-only crates (sm-trigger, sm-diag,
    // sm-updater CLI) are intentionally excluded.
    const GUI_REMOTE_SOURCE: &str = include_str!("remote.rs");
    const GUI_UTIL_SOURCE: &str = include_str!("util.rs");
    const UPDATER_RUNNER_SOURCE: &str = include_str!("../../sm-updater/src/runner.rs");
    const UPDATER_CREDENTIAL_SOURCE: &str = include_str!("../../sm-updater/src/credential.rs");
    const WIN_PROCESS_SOURCE: &str = include_str!("../../sm-win/src/process.rs");

    fn all_sources() -> String {
        format!(
            "{INDEX_HTML}\n{APP}\n{SERVICE_TABLE}\n{SERVICE_FORM}\n{SERVICE_ACTIONS}\n{LOG_PANEL}\n{BATCH_DIALOG}\n{IMPORT_DIALOG}\n{ABOUT_DIALOG}\n{ADMIN_BANNER}\n{THEME_TOGGLE}\n{THEME_PROVIDER}\n{UPDATER_VIEW}\n{UPDATER_FORM}\n{REMOTE_VIEW}\n{SETTINGS_VIEW}\n{TYPES}\n{HINT}\n{TOOLTIP}"
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
            "plan_updater_app",
            "load_updater_history",
            "delete_updater_app",
            "read_updater_log",
            "save_updater_app",
            "load_maintenance_status",
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
        for action in [
            "Preview Plan",
            "Run",
            "Check",
            "Test",
            "Log",
            "History",
            "Edit",
            "Delete",
        ] {
            assert!(
                UPDATER_VIEW.contains(action),
                "missing updater context action: {action}"
            );
        }
        assert!(UPDATER_VIEW.contains("No history yet."));
        assert!(UPDATER_VIEW.contains("Client IP:"));
        assert!(UPDATER_VIEW.contains("Dry run"));
        assert!(UPDATER_VIEW.contains("does not merge"));
        assert!(UPDATER_VIEW.contains("Maintenance mode is enabled"));
        assert!(UPDATER_VIEW.contains("Disable maintenance mode in Settings when ready."));
        assert!(include_str!("main.rs").contains("updater::plan_updater_app"));
        assert!(include_str!("main.rs").contains("updater::load_updater_history"));
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
    fn settings_doctor_ui_has_run_copy_and_status_labels() {
        for label in [
            "Doctor",
            "Run Doctor",
            "Copy Report",
            "Healthy",
            "Warning",
            "Error",
            "Checking",
            "Not run",
            "Generated",
            "Base path",
            "Read-only",
            "Skipped",
            "Remote access",
            "Scheduled Task",
        ] {
            assert!(
                SETTINGS_VIEW.contains(label),
                "missing Doctor label: {label}"
            );
        }
        assert!(SETTINGS_VIEW.contains("run_doctor"));
        assert!(SETTINGS_VIEW.contains("navigator.clipboard"));
        assert!(SETTINGS_VIEW.contains("DoctorChecks"));
        assert!(SETTINGS_VIEW.contains("doctorReportText"));
        assert!(include_str!("main.rs").contains("doctor::run_doctor"));
    }

    #[test]
    fn settings_ui_has_toolchain_and_privileged_task_actions() {
        for label in [
            "Settings",
            "Toolchain",
            "Log retention days",
            "Clean Logs Now",
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
            "Console window",
            "Task action is not hidden",
            "Legacy fallback",
            "Missing required tools",
            "Invalid tool path",
            "Scheduled Task not installed",
            "Trigger",
            "Allowed IPs / CIDR",
            "Allow local trigger",
            "Current client IP",
            "Migrate Trigger",
            "Maintenance Mode",
            "Pause updates and SSH trigger",
            "Maintenance message",
            "Deploy window / maintenance reason",
            "Check and dry-run plan still work",
            "Export Settings…",
            "Load Settings File…",
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
            "PHP references found",
            "Old PHP path",
            "New PHP path",
            "Use new path",
            "Use ${PHP}",
            "Leave",
        ] {
            assert!(
                SETTINGS_VIEW.contains(label),
                "missing settings view label: {label}"
            );
        }
        for command in [
            "load_settings_view",
            "save_settings_view",
            "export_settings_file",
            "load_settings_file",
            "migrate_trigger_to_settings",
            "prune_logs_now",
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
            "replace_php_references",
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
            "maintenance",
            "draft.maintenance.enabled",
            "draft.maintenance.message",
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
            "phpPathChange",
            "oldRoot",
            "newRoot",
            "references",
            "appName",
        ] {
            assert!(
                SETTINGS_VIEW.contains(field),
                "missing settings field: {field}"
            );
        }
        assert!(SETTINGS_VIEW.contains("lucide-react"));
        assert!(SETTINGS_VIEW.contains("Active/recent logs are skipped."));
        assert!(include_str!("main.rs").contains("settings::prune_logs_now"));
        assert!(include_str!("main.rs").contains("settings::export_settings_file"));
        assert!(include_str!("main.rs").contains("settings::load_settings_file"));
        assert!(include_str!("main.rs").contains("settings::load_maintenance_status"));
        assert!(APP.contains("SettingsView"));
        assert!(APP.contains("setView(\"settings\")"));
        assert!(APP.contains("view === \"settings\""));
        assert!(APP.contains("admin={admin}"));
        assert!(APP.contains("lucide-react"));
    }

    #[test]
    fn settings_ui_has_git_credentials_and_readonly_firewall() {
        for label in [
            "Git Credentials",
            "Store Token",
            "Test Remote",
            "Git Credential Manager",
            "DPAPI",
            "not in settings.json",
            "minimum scope",
            "App for Test/Clear",
            "No credential hosts found",
            "Personal Access Token",
        ] {
            assert!(
                SETTINGS_VIEW.contains(label),
                "missing Git credential label: {label}"
            );
        }
        for command in [
            "list_git_credential_hosts",
            "store_git_credential",
            "test_git_credential",
            "clear_git_credential",
        ] {
            assert!(
                SETTINGS_VIEW.contains(command),
                "missing Git credential command: {command}"
            );
        }

        for label in [
            "SSH Firewall (read-only)",
            "Refresh",
            "Display name",
            "Enabled",
            "Action",
            "Profile",
            "Local port",
            "Read-only view. Change rules in Windows Firewall.",
            "Doctor checks port 22",
            "test SSH",
            "No inbound SSH rules found",
        ] {
            assert!(
                SETTINGS_VIEW.contains(label),
                "missing firewall label: {label}"
            );
        }
        assert!(SETTINGS_VIEW.contains("ssh_firewall_status"));
        assert!(TYPES.contains("export interface GitCredentialHostView"));
        assert!(TYPES.contains("export interface FirewallRuleView"));
        assert!(TYPES.contains("export interface FirewallStatusView"));
        assert!(include_str!("main.rs").contains("credentials::list_git_credential_hosts"));
        assert!(include_str!("main.rs").contains("credentials::store_git_credential"));
        assert!(include_str!("main.rs").contains("credentials::test_git_credential"));
        assert!(include_str!("main.rs").contains("credentials::clear_git_credential"));
        assert!(include_str!("main.rs").contains("firewall::read_ssh_firewall_status"));
    }

    fn assert_spawned_commands_are_hidden(name: &str, source: &str) {
        let mut rest = source;
        let mut checked = 0;
        while let Some(index) = rest.find("Command::new") {
            let tail = &rest[index..];
            let boundary = [".spawn()", ".output()", ".status()"]
                .iter()
                .filter_map(|needle| tail.find(needle))
                .min()
                .unwrap_or_else(|| tail.len().min(800));
            let segment = &tail[..boundary.clamp(1, 800)];
            assert!(
                segment.contains(".no_window()"),
                "{name}: Command::new without .no_window() near: {}",
                segment.chars().take(140).collect::<String>()
            );
            checked += 1;
            rest = &tail[boundary.max(1)..];
        }
        assert!(checked > 0, "{name}: expected at least one Command::new");
    }

    #[test]
    fn every_gui_used_child_process_hides_its_console_window() {
        for (name, source) in [
            ("sm-gui/remote.rs", GUI_REMOTE_SOURCE),
            ("sm-gui/util.rs", GUI_UTIL_SOURCE),
            ("sm-updater/runner.rs", UPDATER_RUNNER_SOURCE),
            ("sm-updater/credential.rs", UPDATER_CREDENTIAL_SOURCE),
            ("sm-win/process.rs", WIN_PROCESS_SOURCE),
        ] {
            assert_spawned_commands_are_hidden(name, source);
        }
    }
}
