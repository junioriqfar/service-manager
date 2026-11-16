use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sm_config::{load_projects, ConfigPaths, ProjectsFile};
use sm_core::ToggleMode;

use crate::credential::{store_credential, verify_remote, CredentialRequest, GitCredentialSink};
use crate::ip::{app_allowed_for_ip, client_ip_from_env, ip_allowed};
use crate::registry::{load_config, save_config, AppConfig, PackageManager, Preset, UpdaterConfig};
use crate::runner::{Runner, SystemRunner};
use crate::scan::{discover_projects, ProjectInfo};
use crate::state::load_state;
use crate::trigger::{run_registry, RunOptions};
use crate::update::{update_project, UpdateOptions, UpdateStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Menu,
    List,
    Check,
    Update,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub config: Option<PathBuf>,
    pub mode: Mode,
    pub project: Option<String>,
    pub all: bool,
    pub help: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            config: None,
            mode: Mode::Menu,
            project: None,
            all: false,
            help: false,
        }
    }
}

pub fn parse_args(values: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut index = 0usize;

    while index < values.len() {
        let value = values[index].as_str();
        match value {
            "--help" | "-h" => args.help = true,
            "--all" => args.all = true,
            "--menu" => args.mode = Mode::Menu,
            "menu" => args.mode = Mode::Menu,
            "list" => args.mode = Mode::List,
            "check" => args.mode = Mode::Check,
            "update" => args.mode = Mode::Update,
            "--config" => {
                index += 1;
                let Some(path) = values.get(index) else {
                    return Err("--config membutuhkan path.".to_string());
                };
                args.config = Some(PathBuf::from(path));
            }
            "--project" => {
                index += 1;
                let Some(project) = values.get(index) else {
                    return Err("--project membutuhkan nama project.".to_string());
                };
                args.project = Some(project.clone());
            }
            other => return Err(format!("Argumen tidak dikenal: {other}")),
        }
        index += 1;
    }

    Ok(args)
}

pub fn run(values: &[String]) -> i32 {
    match values.first().map(String::as_str) {
        Some("app") => return run_app(&values[1..]),
        Some("run") => return run_trigger(&values[1..]),
        Some("status") => return run_status(&values[1..]),
        Some("authorize") => return run_authorize(&values[1..]),
        Some("logs") => return run_logs(&values[1..]),
        Some("credential") => return run_credential(&values[1..]),
        _ => {}
    }

    let args = match parse_args(values) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            return 2;
        }
    };

    if args.help {
        print_usage();
        return 0;
    }

    let config_path = args
        .config
        .clone()
        .unwrap_or_else(|| ConfigPaths::detect().projects_path());
    let settings = match load_projects(&config_path) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("Config tidak valid: {error}");
            return 1;
        }
    };

    let log_dir = log_dir(&settings, &config_path);
    let projects = discover_projects(&settings);

    match args.mode {
        Mode::List => {
            list_projects(&projects);
            0
        }
        Mode::Menu => run_menu(&projects, &settings, &log_dir),
        Mode::Check | Mode::Update => run_targets(&args, &projects, &settings, &log_dir),
    }
}

fn default_config_path() -> PathBuf {
    ConfigPaths::detect().projects_path()
}

/// Terapkan `settings.json` (jika ada) ke config registry. `settings.json`
/// authoritative; field `settings` di `projects.json` hanya fallback.
fn apply_settings(config_path: &Path, config: &mut UpdaterConfig) -> Result<(), String> {
    let path = crate::settings::settings_path_for(config_path);
    if let Some(settings) =
        crate::settings::load_settings(&path).map_err(|error| error.to_string())?
    {
        config.settings = settings.to_updater_settings();
    }
    Ok(())
}

fn take_value(values: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    values
        .get(*index)
        .cloned()
        .ok_or_else(|| format!("{flag} membutuhkan nilai."))
}

fn parse_toggle(value: &str) -> Result<ToggleMode, String> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(ToggleMode::Auto),
        "always" => Ok(ToggleMode::Always),
        "never" => Ok(ToggleMode::Never),
        _ => Err(format!(
            "Nilai toggle tidak valid: {value} (auto/always/never)"
        )),
    }
}

fn parse_preset(value: &str) -> Result<Preset, String> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(Preset::Auto),
        "laravel" => Ok(Preset::Laravel),
        "node" => Ok(Preset::Node),
        "bun" => Ok(Preset::Bun),
        "go" => Ok(Preset::Go),
        "static" => Ok(Preset::Static),
        "custom" => Ok(Preset::Custom),
        _ => Err(format!("Preset tidak valid: {value}")),
    }
}

fn parse_package_manager(value: &str) -> Result<PackageManager, String> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(PackageManager::Auto),
        "npm" => Ok(PackageManager::Npm),
        "pnpm" => Ok(PackageManager::Pnpm),
        "yarn" => Ok(PackageManager::Yarn),
        "bun" => Ok(PackageManager::Bun),
        "none" => Ok(PackageManager::None),
        _ => Err(format!("Package manager tidak valid: {value}")),
    }
}

fn run_app(values: &[String]) -> i32 {
    let Some(command) = values.first().map(String::as_str) else {
        eprintln!("Gunakan: sm-updater app <add|list|show|remove|test>");
        return 2;
    };

    match command {
        "add" => app_add(&values[1..]),
        "edit" => app_edit(&values[1..]),
        "list" => app_list(&values[1..]),
        "show" => app_show(&values[1..]),
        "remove" => app_remove(&values[1..]),
        "test" => app_test(&values[1..]),
        "help" | "-h" => {
            print_usage();
            0
        }
        other => {
            eprintln!("Subcommand app tidak dikenal: {other}");
            2
        }
    }
}

fn app_add(values: &[String]) -> i32 {
    let mut config_path = default_config_path();
    let mut id = String::new();
    let mut name = String::new();
    let mut repo_url = String::new();
    let mut local_path = String::new();
    let mut branch = "main".to_string();
    let mut preset = Preset::Auto;
    let mut package_manager = PackageManager::Auto;
    let mut composer = ToggleMode::Auto;
    let mut artisan = ToggleMode::Auto;
    let mut package_install = ToggleMode::Auto;
    let mut build = ToggleMode::Auto;
    let mut queue_restart = true;
    let mut install_command = String::new();
    let mut build_command = String::new();
    let mut pre_update_command = String::new();
    let mut post_update_command = String::new();
    let mut stop_before_pull: Vec<String> = Vec::new();
    let mut stop_before_build: Vec<String> = Vec::new();
    let mut restart_after: Vec<String> = Vec::new();
    let mut start_after: Vec<String> = Vec::new();
    let mut wait_timeout_seconds = 60u64;
    let mut allowed_ips: Vec<String> = Vec::new();
    let mut enabled = true;
    let mut index = 0usize;

    while index < values.len() {
        match values[index].as_str() {
            "--config" => match take_value(values, &mut index, "--config") {
                Ok(value) => config_path = PathBuf::from(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--id" => match take_value(values, &mut index, "--id") {
                Ok(value) => id = value,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--name" => match take_value(values, &mut index, "--name") {
                Ok(value) => name = value,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--repo" => match take_value(values, &mut index, "--repo") {
                Ok(value) => repo_url = value,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--path" => match take_value(values, &mut index, "--path") {
                Ok(value) => local_path = value,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--branch" => match take_value(values, &mut index, "--branch") {
                Ok(value) => branch = value,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--preset" => {
                match take_value(values, &mut index, "--preset")
                    .and_then(|value| parse_preset(&value))
                {
                    Ok(value) => preset = value,
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--package-manager" => match take_value(values, &mut index, "--package-manager")
                .and_then(|value| parse_package_manager(&value))
            {
                Ok(value) => package_manager = value,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--composer" => {
                match take_value(values, &mut index, "--composer")
                    .and_then(|value| parse_toggle(&value))
                {
                    Ok(value) => composer = value,
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--build" => {
                match take_value(values, &mut index, "--build")
                    .and_then(|value| parse_toggle(&value))
                {
                    Ok(value) => build = value,
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--artisan" => {
                match take_value(values, &mut index, "--artisan")
                    .and_then(|value| parse_toggle(&value))
                {
                    Ok(value) => artisan = value,
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--package-install" => match take_value(values, &mut index, "--package-install")
                .and_then(|value| parse_toggle(&value))
            {
                Ok(value) => package_install = value,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--no-queue-restart" => queue_restart = false,
            "--queue-restart" => queue_restart = true,
            "--pre-update-command" => {
                match take_value(values, &mut index, "--pre-update-command") {
                    Ok(value) => pre_update_command = value,
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--post-update-command" => {
                match take_value(values, &mut index, "--post-update-command") {
                    Ok(value) => post_update_command = value,
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--stop-before-pull" => match take_value(values, &mut index, "--stop-before-pull") {
                Ok(value) => stop_before_pull.push(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--stop-before-build" => match take_value(values, &mut index, "--stop-before-build") {
                Ok(value) => stop_before_build.push(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--restart-after" => match take_value(values, &mut index, "--restart-after") {
                Ok(value) => restart_after.push(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--start-after" => match take_value(values, &mut index, "--start-after") {
                Ok(value) => start_after.push(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--wait-timeout" => {
                match take_value(values, &mut index, "--wait-timeout").and_then(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|_| "wait-timeout harus angka detik.".to_string())
                }) {
                    Ok(value) => wait_timeout_seconds = value,
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--build-command" => match take_value(values, &mut index, "--build-command") {
                Ok(value) => build_command = value,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--install-command" => match take_value(values, &mut index, "--install-command") {
                Ok(value) => install_command = value,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--allow-ip" | "--allow-cidr" => match take_value(values, &mut index, "--allow-ip") {
                Ok(value) => allowed_ips.push(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--disabled" => enabled = false,
            other => {
                eprintln!("Argumen app add tidak dikenal: {other}");
                return 2;
            }
        }
        index += 1;
    }

    if id.trim().is_empty() || repo_url.trim().is_empty() || local_path.trim().is_empty() {
        eprintln!("--id, --repo, dan --path wajib diisi.");
        return 2;
    }
    if !Path::new(&local_path).is_dir() {
        eprintln!("Local path tidak ditemukan: {local_path}");
        return 1;
    }
    if name.trim().is_empty() {
        name = id.clone();
    }

    let mut config = match load_config(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    if let Err(error) = apply_settings(&config_path, &mut config) {
        eprintln!("{error}");
        return 1;
    }
    if config.settings.log_dir.trim().is_empty() {
        config.settings.log_dir = crate::trigger::default_log_dir(&config_path)
            .to_string_lossy()
            .to_string();
    }
    if config.find(&id).is_some() {
        eprintln!("App '{id}' sudah terdaftar.");
        return 1;
    }

    let app = AppConfig {
        id: id.clone(),
        name,
        enabled,
        repo_url,
        local_path,
        branch,
        preset,
        package_manager,
        composer,
        artisan,
        package_install,
        build,
        queue_restart,
        install_command,
        build_command,
        pre_update_command,
        post_update_command,
        services: crate::registry::ServicePlan {
            stop_before_pull,
            stop_before_build,
            restart_after,
            start_after,
            wait_timeout_seconds,
        },
        allowed_client_ips: allowed_ips,
        ..AppConfig::default()
    };
    if let Err(error) = config.validate_app(&app) {
        eprintln!("{error}");
        return 1;
    }
    config.apps.push(app);
    if let Err(error) = save_config(&config_path, &config) {
        eprintln!("{error}");
        return 1;
    }
    println!("App '{id}' ditambahkan ke {}", config_path.display());
    0
}

#[derive(Default)]
struct EditOptions {
    name: Option<String>,
    repo_url: Option<String>,
    local_path: Option<String>,
    branch: Option<String>,
    preset: Option<Preset>,
    package_manager: Option<PackageManager>,
    composer: Option<ToggleMode>,
    artisan: Option<ToggleMode>,
    package_install: Option<ToggleMode>,
    build: Option<ToggleMode>,
    queue_restart: Option<bool>,
    install_command: Option<String>,
    build_command: Option<String>,
    pre_update_command: Option<String>,
    post_update_command: Option<String>,
    allowed_ips: Option<Vec<String>>,
    clear_allow_ips: bool,
    stop_before_pull: Option<Vec<String>>,
    stop_before_build: Option<Vec<String>>,
    restart_after: Option<Vec<String>>,
    start_after: Option<Vec<String>>,
    wait_timeout: Option<u64>,
    enabled: Option<bool>,
}

fn app_edit(values: &[String]) -> i32 {
    let mut config_path = default_config_path();
    let mut id = String::new();
    let mut options = EditOptions::default();
    let mut index = 0usize;

    while index < values.len() {
        match values[index].as_str() {
            "--config" => match take_value(values, &mut index, "--config") {
                Ok(value) => config_path = PathBuf::from(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--name" => match take_value(values, &mut index, "--name") {
                Ok(value) => options.name = Some(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--repo" => match take_value(values, &mut index, "--repo") {
                Ok(value) => options.repo_url = Some(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--path" => match take_value(values, &mut index, "--path") {
                Ok(value) => options.local_path = Some(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--branch" => match take_value(values, &mut index, "--branch") {
                Ok(value) => options.branch = Some(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--preset" => {
                match take_value(values, &mut index, "--preset")
                    .and_then(|value| parse_preset(&value))
                {
                    Ok(value) => options.preset = Some(value),
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--package-manager" => match take_value(values, &mut index, "--package-manager")
                .and_then(|value| parse_package_manager(&value))
            {
                Ok(value) => options.package_manager = Some(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--composer" => {
                match take_value(values, &mut index, "--composer")
                    .and_then(|value| parse_toggle(&value))
                {
                    Ok(value) => options.composer = Some(value),
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--artisan" => {
                match take_value(values, &mut index, "--artisan")
                    .and_then(|value| parse_toggle(&value))
                {
                    Ok(value) => options.artisan = Some(value),
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--package-install" => match take_value(values, &mut index, "--package-install")
                .and_then(|value| parse_toggle(&value))
            {
                Ok(value) => options.package_install = Some(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--build" => {
                match take_value(values, &mut index, "--build")
                    .and_then(|value| parse_toggle(&value))
                {
                    Ok(value) => options.build = Some(value),
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--queue-restart" => options.queue_restart = Some(true),
            "--no-queue-restart" => options.queue_restart = Some(false),
            "--install-command" => match take_value(values, &mut index, "--install-command") {
                Ok(value) => options.install_command = Some(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--build-command" => match take_value(values, &mut index, "--build-command") {
                Ok(value) => options.build_command = Some(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--pre-update-command" => {
                match take_value(values, &mut index, "--pre-update-command") {
                    Ok(value) => options.pre_update_command = Some(value),
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--post-update-command" => {
                match take_value(values, &mut index, "--post-update-command") {
                    Ok(value) => options.post_update_command = Some(value),
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--allow-ip" | "--allow-cidr" => match take_value(values, &mut index, "--allow-ip") {
                Ok(value) => options.allowed_ips.get_or_insert_with(Vec::new).push(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--clear-allow-ips" => options.clear_allow_ips = true,
            "--stop-before-pull" => match take_value(values, &mut index, "--stop-before-pull") {
                Ok(value) => options
                    .stop_before_pull
                    .get_or_insert_with(Vec::new)
                    .push(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--stop-before-build" => match take_value(values, &mut index, "--stop-before-build") {
                Ok(value) => options
                    .stop_before_build
                    .get_or_insert_with(Vec::new)
                    .push(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--restart-after" => match take_value(values, &mut index, "--restart-after") {
                Ok(value) => options
                    .restart_after
                    .get_or_insert_with(Vec::new)
                    .push(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--start-after" => match take_value(values, &mut index, "--start-after") {
                Ok(value) => options.start_after.get_or_insert_with(Vec::new).push(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--wait-timeout" => {
                match take_value(values, &mut index, "--wait-timeout").and_then(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|_| "wait-timeout harus angka detik.".to_string())
                }) {
                    Ok(value) => options.wait_timeout = Some(value),
                    Err(error) => {
                        eprintln!("{error}");
                        return 2;
                    }
                }
            }
            "--enable" => options.enabled = Some(true),
            "--disable" => options.enabled = Some(false),
            value if id.is_empty() => id = value.to_string(),
            other => {
                eprintln!("Argumen app edit tidak dikenal: {other}");
                return 2;
            }
        }
        index += 1;
    }

    if id.is_empty() {
        eprintln!("Gunakan: sm-updater app edit ID [opsi]");
        return 2;
    }

    let mut config = match load_config(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let Some(app) = config.find_mut(&id) else {
        eprintln!("App '{id}' tidak ditemukan.");
        return 1;
    };

    if let Some(value) = options.name {
        app.name = value;
    }
    if let Some(value) = options.repo_url {
        app.repo_url = value;
    }
    if let Some(value) = options.local_path {
        app.local_path = value;
    }
    if let Some(value) = options.branch {
        app.branch = value;
    }
    if let Some(value) = options.preset {
        app.preset = value;
    }
    if let Some(value) = options.package_manager {
        app.package_manager = value;
    }
    if let Some(value) = options.composer {
        app.composer = value;
    }
    if let Some(value) = options.artisan {
        app.artisan = value;
    }
    if let Some(value) = options.package_install {
        app.package_install = value;
    }
    if let Some(value) = options.build {
        app.build = value;
    }
    if let Some(value) = options.queue_restart {
        app.queue_restart = value;
    }
    if let Some(value) = options.install_command {
        app.install_command = value;
    }
    if let Some(value) = options.build_command {
        app.build_command = value;
    }
    if let Some(value) = options.pre_update_command {
        app.pre_update_command = value;
    }
    if let Some(value) = options.post_update_command {
        app.post_update_command = value;
    }
    if options.clear_allow_ips {
        app.allowed_client_ips.clear();
    }
    if let Some(value) = options.allowed_ips {
        app.allowed_client_ips = value;
    }
    if let Some(value) = options.stop_before_pull {
        app.services.stop_before_pull = value;
    }
    if let Some(value) = options.stop_before_build {
        app.services.stop_before_build = value;
    }
    if let Some(value) = options.restart_after {
        app.services.restart_after = value;
    }
    if let Some(value) = options.start_after {
        app.services.start_after = value;
    }
    if let Some(value) = options.wait_timeout {
        app.services.wait_timeout_seconds = value;
    }
    if let Some(value) = options.enabled {
        app.enabled = value;
    }

    app.normalize();
    let edited = app.clone();
    if let Err(error) = config.validate_app(&edited) {
        eprintln!("{error}");
        return 1;
    }
    if let Err(error) = save_config(&config_path, &config) {
        eprintln!("{error}");
        return 1;
    }
    println!("App '{id}' diperbarui di {}", config_path.display());
    0
}

fn app_list(values: &[String]) -> i32 {
    let mut config_path = default_config_path();
    let mut index = 0usize;
    while index < values.len() {
        if values[index] == "--config" {
            match take_value(values, &mut index, "--config") {
                Ok(value) => config_path = PathBuf::from(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            }
        } else {
            eprintln!("Argumen app list tidak dikenal: {}", values[index]);
            return 2;
        }
        index += 1;
    }

    let config = match load_config(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    if config.apps.is_empty() {
        println!("Belum ada app terdaftar.");
        return 0;
    }
    println!(
        "{:<20} {:<10} {:<10} {:<30} PATH",
        "ID", "ENABLED", "BRANCH", "REPO"
    );
    for app in &config.apps {
        println!(
            "{:<20} {:<10} {:<10} {:<30} {}",
            app.id,
            if app.enabled { "yes" } else { "no" },
            app.branch,
            app.repo_url,
            app.local_path
        );
    }
    0
}

fn app_show(values: &[String]) -> i32 {
    let mut config_path = default_config_path();
    let mut id = String::new();
    let mut index = 0usize;
    while index < values.len() {
        match values[index].as_str() {
            "--config" => match take_value(values, &mut index, "--config") {
                Ok(value) => config_path = PathBuf::from(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            value if id.is_empty() => id = value.to_string(),
            other => {
                eprintln!("Argumen app show tidak dikenal: {other}");
                return 2;
            }
        }
        index += 1;
    }
    let config = match load_config(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let Some(app) = config.find(&id) else {
        eprintln!("App '{id}' tidak ditemukan.");
        return 1;
    };
    match serde_json::to_string_pretty(app) {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

fn app_remove(values: &[String]) -> i32 {
    let mut config_path = default_config_path();
    let mut id = String::new();
    let mut index = 0usize;
    while index < values.len() {
        match values[index].as_str() {
            "--config" => match take_value(values, &mut index, "--config") {
                Ok(value) => config_path = PathBuf::from(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            value if id.is_empty() => id = value.to_string(),
            other => {
                eprintln!("Argumen app remove tidak dikenal: {other}");
                return 2;
            }
        }
        index += 1;
    }
    let mut config = match load_config(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let before = config.apps.len();
    config.apps.retain(|app| !app.id.eq_ignore_ascii_case(&id));
    if config.apps.len() == before {
        eprintln!("App '{id}' tidak ditemukan.");
        return 1;
    }
    if let Err(error) = save_config(&config_path, &config) {
        eprintln!("{error}");
        return 1;
    }
    println!("App '{id}' dihapus dari registry.");
    0
}

fn app_test(values: &[String]) -> i32 {
    let mut config_path = default_config_path();
    let mut id = String::new();
    let mut index = 0usize;
    while index < values.len() {
        match values[index].as_str() {
            "--config" => match take_value(values, &mut index, "--config") {
                Ok(value) => config_path = PathBuf::from(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            value if id.is_empty() => id = value.to_string(),
            other => {
                eprintln!("Argumen app test tidak dikenal: {other}");
                return 2;
            }
        }
        index += 1;
    }
    let mut config = match load_config(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    if let Err(error) = apply_settings(&config_path, &mut config) {
        eprintln!("{error}");
        return 1;
    }
    let Some(app) = config.find(&id) else {
        eprintln!("App '{id}' tidak ditemukan.");
        return 1;
    };
    if let Err(error) = config.validate_app(app) {
        eprintln!("{error}");
        return 1;
    }
    let path = Path::new(&app.local_path);
    if !path.is_dir() {
        eprintln!("Local path tidak ditemukan: {}", path.display());
        return 1;
    }
    let settings = crate::registry::project_settings(&config);
    if !Path::new(&settings.git_path).is_file() {
        eprintln!("Git tidak ditemukan: {}", settings.git_path);
        return 1;
    }

    let runner = SystemRunner;
    let branch = runner.run(
        Path::new(&settings.git_path),
        &[
            "-C".to_string(),
            app.local_path.clone(),
            "rev-parse".to_string(),
            "--abbrev-ref".to_string(),
            "HEAD".to_string(),
        ],
        path,
        Duration::from_secs(30),
    );
    match branch {
        Ok(output) if output.success() => {
            println!("Branch aktif : {}", output.stdout.trim());
        }
        Ok(output) => {
            eprintln!("Gagal membaca branch: {}", output.combined());
            return 1;
        }
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    }

    let remote = runner.run(
        Path::new(&settings.git_path),
        &[
            "-C".to_string(),
            app.local_path.clone(),
            "ls-remote".to_string(),
            "--heads".to_string(),
            "origin".to_string(),
        ],
        path,
        Duration::from_secs(30),
    );
    match remote {
        Ok(output) if output.success() => println!("Remote       : OK"),
        Ok(output) => {
            eprintln!("Remote gagal: {}", output.combined());
            return 1;
        }
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    }

    for marker in [
        "composer.json",
        "composer.lock",
        "artisan",
        "package.json",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
        "go.mod",
    ] {
        if path.join(marker).is_file() {
            println!("Detected     : {marker}");
        }
    }
    println!("App '{}' valid.", app.id);
    0
}

fn run_trigger(values: &[String]) -> i32 {
    let mut config_path = default_config_path();
    let mut app = None;
    let mut client_ip = None;
    let mut check_only = false;
    let mut index = 0usize;
    while index < values.len() {
        match values[index].as_str() {
            "--config" => match take_value(values, &mut index, "--config") {
                Ok(value) => config_path = PathBuf::from(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--app" => match take_value(values, &mut index, "--app") {
                Ok(value) => app = Some(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--client-ip" => match take_value(values, &mut index, "--client-ip").and_then(|value| {
                value
                    .parse()
                    .map_err(|_| format!("IP tidak valid: {value}"))
            }) {
                Ok(value) => client_ip = Some(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--check" => check_only = true,
            "--all" => {}
            other => {
                eprintln!("Argumen run tidak dikenal: {other}");
                return 2;
            }
        }
        index += 1;
    }

    let config = match load_config(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let runner = SystemRunner;
    let outcomes = match run_registry(
        &config,
        &config_path,
        &runner,
        RunOptions {
            check_only,
            app: app.clone(),
            client_ip,
        },
    ) {
        Ok(outcomes) => outcomes,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };

    let mut failed = 0;
    let mut forbidden = 0;
    for outcome in &outcomes {
        println!(
            "{}: {} - {}",
            outcome.app_id, outcome.status, outcome.message
        );
        if outcome.forbidden {
            forbidden += 1;
        } else if outcome.status == "failed" {
            failed += 1;
        }
    }
    if failed > 0 {
        1
    } else if forbidden > 0 {
        3
    } else {
        0
    }
}

fn run_authorize(values: &[String]) -> i32 {
    let mut config_path = default_config_path();
    let mut app = None;
    let mut index = 0usize;
    while index < values.len() {
        match values[index].as_str() {
            "--config" => match take_value(values, &mut index, "--config") {
                Ok(value) => config_path = PathBuf::from(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--app" => match take_value(values, &mut index, "--app") {
                Ok(value) => app = Some(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            other => {
                eprintln!("Argumen authorize tidak dikenal: {other}");
                return 2;
            }
        }
        index += 1;
    }

    let config = match load_config(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let client_ip = client_ip_from_env();

    let (allowed, label) = if let Some(id) = app {
        let Some(app) = config.find(&id) else {
            eprintln!("App '{id}' tidak ditemukan.");
            return 1;
        };
        (
            app_allowed_for_ip(&config, app, client_ip),
            format!("app '{id}'"),
        )
    } else {
        let allowed = match client_ip {
            Some(ip) => {
                config.trigger.allowed_client_ips.is_empty()
                    || ip_allowed(ip, &config.trigger.allowed_client_ips)
            }
            None => config.trigger.allow_local,
        };
        (allowed, "trigger global".to_string())
    };

    if allowed {
        match client_ip {
            Some(ip) => println!("Trigger diizinkan untuk {label} dari {ip}."),
            None => println!("Trigger lokal diizinkan untuk {label}."),
        }
        0
    } else {
        match client_ip {
            Some(ip) => eprintln!("Trigger ditolak untuk {label}: IP {ip} tidak ada di allowlist."),
            None => eprintln!("Trigger ditolak untuk {label}: trigger lokal dinonaktifkan."),
        }
        3
    }
}

fn run_status(values: &[String]) -> i32 {
    let mut config_path = default_config_path();
    let mut index = 0usize;
    while index < values.len() {
        match values[index].as_str() {
            "--config" => match take_value(values, &mut index, "--config") {
                Ok(value) => config_path = PathBuf::from(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            other => {
                eprintln!("Argumen status tidak dikenal: {other}");
                return 2;
            }
        }
        index += 1;
    }
    let state = load_state(&config_path);
    if state.apps.is_empty() {
        println!("Belum ada state updater.");
        return 0;
    }
    for (id, app) in &state.apps {
        println!(
            "{:<20} {:<15} checked={} updated={} error={}",
            id, app.status, app.last_checked_at, app.last_updated_at, app.last_error
        );
    }
    0
}

fn run_logs(values: &[String]) -> i32 {
    let mut config_path = default_config_path();
    let mut id = String::new();
    let mut tail = 50usize;
    let mut index = 0usize;
    while index < values.len() {
        match values[index].as_str() {
            "--config" => match take_value(values, &mut index, "--config") {
                Ok(value) => config_path = PathBuf::from(value),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            "--tail" => match take_value(values, &mut index, "--tail").and_then(|value| {
                value
                    .parse::<usize>()
                    .map_err(|_| "--tail harus angka.".to_string())
            }) {
                Ok(value) => tail = value,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            },
            value if id.is_empty() => id = value.to_string(),
            other => {
                eprintln!("Argumen logs tidak dikenal: {other}");
                return 2;
            }
        }
        index += 1;
    }

    let mut config = match load_config(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    if let Err(error) = apply_settings(&config_path, &mut config) {
        eprintln!("{error}");
        return 1;
    }

    if id.is_empty() {
        let log_dir = if config.settings.log_dir.trim().is_empty() {
            crate::trigger::default_log_dir(&config_path)
        } else {
            PathBuf::from(config.settings.log_dir.trim())
        };
        let logs = recent_logs(&log_dir, 10);
        if logs.is_empty() {
            println!("Belum ada log di {}", log_dir.display());
            return 0;
        }
        for log in logs {
            println!("{}", log.display());
        }
        return 0;
    }

    let Some(app) = config.find(&id) else {
        eprintln!("App '{id}' tidak ditemukan.");
        return 1;
    };
    let project_logs = Path::new(&app.local_path).join("storage/logs");
    let Some(file) = latest_log(&project_logs) else {
        eprintln!(
            "Belum ada log project untuk '{}' di {}.",
            app.id,
            project_logs.display()
        );
        return 1;
    };
    println!("{}", file.display());
    match std::fs::read_to_string(&file) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(tail);
            for line in &lines[start..] {
                println!("{line}");
            }
            0
        }
        Err(error) => {
            eprintln!("Gagal membaca log: {error}");
            1
        }
    }
}

fn recent_logs(dir: &Path, limit: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("project-update-") && name.ends_with(".log"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files.reverse();
    files.truncate(limit);
    files
}

fn latest_log(dir: &Path) -> Option<PathBuf> {
    recent_logs(dir, 1).into_iter().next()
}

fn log_dir(settings: &ProjectsFile, config_path: &std::path::Path) -> PathBuf {
    if settings.log_dir.trim().is_empty() {
        config_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("logs")
    } else {
        PathBuf::from(settings.log_dir.trim())
    }
}

fn list_projects(projects: &[ProjectInfo]) {
    if projects.is_empty() {
        println!("Tidak ada project Laravel yang terdeteksi.");
        return;
    }
    println!("Project Laravel:");
    for project in projects {
        let state = if project.config.enabled {
            "aktif"
        } else {
            "nonaktif"
        };
        println!(
            "  {} [{}] ({}) -> {}",
            project.name,
            project.config.branch,
            state,
            project.path.display()
        );
    }
}

fn run_targets(
    args: &Args,
    projects: &[ProjectInfo],
    settings: &ProjectsFile,
    log_dir: &std::path::Path,
) -> i32 {
    let targets: Vec<&ProjectInfo> = if let Some(name) = &args.project {
        let found: Vec<&ProjectInfo> = projects
            .iter()
            .filter(|project| project.name.eq_ignore_ascii_case(name))
            .collect();
        if found.is_empty() {
            eprintln!("Project '{name}' tidak ditemukan.");
            return 1;
        }
        found
    } else {
        projects
            .iter()
            .filter(|project| project.config.enabled)
            .collect()
    };

    if targets.is_empty() {
        eprintln!("Tidak ada project aktif.");
        return 1;
    }

    let runner = SystemRunner;
    let options = UpdateOptions {
        check_only: args.mode == Mode::Check,
    };
    let mut failed = 0;
    for project in targets {
        let outcome = update_project(project, settings, log_dir, &runner, options);
        println!(
            "\n{}: {} - {}",
            outcome.project,
            outcome.status.as_str(),
            outcome.message
        );
        if outcome.status == UpdateStatus::Failed {
            failed += 1;
        }
    }

    if failed > 0 {
        1
    } else {
        0
    }
}

fn run_menu(projects: &[ProjectInfo], settings: &ProjectsFile, log_dir: &std::path::Path) -> i32 {
    if projects.is_empty() {
        println!("Tidak ada project Laravel yang terdeteksi.");
        return 0;
    }

    loop {
        println!();
        println!("============================================================");
        println!(" PROJECT UPDATE");
        println!("============================================================");
        for (index, project) in projects.iter().enumerate() {
            let state = if project.config.enabled {
                "aktif"
            } else {
                "nonaktif"
            };
            println!(
                "  {}. {} [{}] ({})",
                index + 1,
                project.name,
                project.config.branch,
                state
            );
        }
        println!("  0. Keluar");

        let Some(choice) = read_line("Pilih project: ") else {
            return 0;
        };
        if choice.is_empty() || choice == "0" {
            return 0;
        }
        let Ok(index) = choice.parse::<usize>() else {
            println!("Pilihan tidak valid.");
            continue;
        };
        if index == 0 || index > projects.len() {
            println!("Pilihan tidak valid.");
            continue;
        }

        let project = &projects[index - 1];
        println!();
        println!("Project: {}", project.name);
        println!("Path   : {}", project.path.display());
        println!("Branch : {}", project.config.branch);
        println!("  1. Cek update (tanpa mengubah file)");
        println!("  2. Update (pull + composer + build)");
        println!("  0. Batal");

        let Some(action) = read_line("Pilih aksi: ") else {
            return 0;
        };
        let check_only = match action.as_str() {
            "1" => true,
            "2" => false,
            _ => continue,
        };

        if !check_only {
            let Some(confirm) = read_line(&format!("Update {} sekarang? Ketik YA: ", project.name))
            else {
                return 0;
            };
            if confirm != "YA" {
                println!("Dibatalkan.");
                continue;
            }
        }

        let runner = SystemRunner;
        let outcome = update_project(
            project,
            settings,
            log_dir,
            &runner,
            UpdateOptions { check_only },
        );
        println!(
            "\n{}: {} - {}",
            outcome.project,
            outcome.status.as_str(),
            outcome.message
        );
        let _ = read_line("Tekan Enter untuk kembali ke menu");
    }
}

fn read_line(prompt: &str) -> Option<String> {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut buffer = String::new();
    let read = io::stdin().read_line(&mut buffer).ok()?;
    if read == 0 {
        return None;
    }
    Some(buffer.trim().to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialArgs {
    pub host: String,
    pub username: String,
    pub project: PathBuf,
    pub git: Option<PathBuf>,
    pub help: bool,
}

impl Default for CredentialArgs {
    fn default() -> Self {
        Self {
            host: "gitlab.com".to_string(),
            username: "oauth2".to_string(),
            project: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            git: None,
            help: false,
        }
    }
}

pub fn parse_credential_args(values: &[String]) -> Result<CredentialArgs, String> {
    let mut args = CredentialArgs::default();
    let mut index = 0usize;
    let mut saw_store = false;

    while index < values.len() {
        match values[index].as_str() {
            "store" => saw_store = true,
            "--help" | "-h" => args.help = true,
            "--host" => {
                index += 1;
                args.host = values
                    .get(index)
                    .ok_or_else(|| "--host membutuhkan nilai.".to_string())?
                    .clone();
            }
            "--username" => {
                index += 1;
                args.username = values
                    .get(index)
                    .ok_or_else(|| "--username membutuhkan nilai.".to_string())?
                    .clone();
            }
            "--project" => {
                index += 1;
                args.project = PathBuf::from(
                    values
                        .get(index)
                        .ok_or_else(|| "--project membutuhkan path.".to_string())?,
                );
            }
            "--git" => {
                index += 1;
                args.git = Some(PathBuf::from(
                    values
                        .get(index)
                        .ok_or_else(|| "--git membutuhkan path.".to_string())?,
                ));
            }
            other => return Err(format!("Argumen credential tidak dikenal: {other}")),
        }
        index += 1;
    }

    if !saw_store && !args.help {
        return Err("Gunakan 'sm-updater credential store'.".to_string());
    }
    Ok(args)
}

fn resolve_git_path() -> Option<PathBuf> {
    if let Ok(value) = std::env::var("SERVICE_MANAGER_GIT") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Some(path);
        }
    }
    for candidate in [
        r"C:\laragon\bin\git\cmd\git.exe",
        r"C:\Program Files\Git\cmd\git.exe",
    ] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    let output = std::process::Command::new("where")
        .arg("git")
        .output()
        .ok()?;
    let first = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let path = PathBuf::from(first);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

fn run_credential(values: &[String]) -> i32 {
    let args = match parse_credential_args(values) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    if args.help {
        print_usage();
        return 0;
    }

    let Some(git_path) = args.git.clone().or_else(resolve_git_path) else {
        eprintln!("Git tidak ditemukan. Gunakan --git PATH.");
        return 1;
    };
    if !git_path.is_file() {
        eprintln!("Git tidak ditemukan: {}", git_path.display());
        return 1;
    }
    if !args.project.is_dir() {
        eprintln!("Folder project tidak ditemukan: {}", args.project.display());
        return 1;
    }
    if args.host.trim().is_empty() {
        eprintln!("Host Git wajib diisi.");
        return 1;
    }
    if args.username.trim().is_empty() {
        eprintln!("Username Git wajib diisi.");
        return 1;
    }

    println!(
        "User   : {}",
        std::env::var("USERNAME").unwrap_or_else(|_| "(unknown)".to_string())
    );
    println!(
        "Server : {}",
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "(unknown)".to_string())
    );
    println!(
        "Menyimpan kredensial Git untuk {} (akun {}).",
        args.host, args.username
    );
    println!("Gunakan Personal Access Token dengan akses read_repository.");

    let token = match rpassword::prompt_password("Token: ") {
        Ok(token) => token,
        Err(error) => {
            eprintln!("Gagal membaca token: {error}");
            return 1;
        }
    };

    let request = CredentialRequest {
        host: args.host,
        username: args.username,
        token,
        git_path,
        project_path: args.project,
    };

    if let Err(error) = store_credential(&request, &GitCredentialSink) {
        eprintln!("{error}");
        return 1;
    }
    if let Err(error) = verify_remote(&request) {
        eprintln!("Kredensial tersimpan, tetapi verifikasi remote gagal:");
        eprintln!("{error}");
        return 1;
    }

    println!("Kredensial berhasil disimpan dan remote bisa diakses.");
    0
}

fn print_usage() {
    println!(
        "Service Manager Project Updater

Penggunaan:
  sm-updater                          Menu interaktif (mode lama)
  sm-updater list                     Daftar project (mode lama)
  sm-updater check [--project NAMA]   Cek update (dry-run, mode lama)
  sm-updater update [--project NAMA]  Update project (mode lama)
  sm-updater --all                    Proses semua project aktif (mode lama)

Registry:
  sm-updater app add                  Daftarkan app
      --id ID --repo URL --path PATH [--branch main]
      [--name NAMA] [--preset auto|laravel|node|bun|go|static|custom]
      [--package-manager auto|npm|pnpm|yarn|bun|none]
      [--composer auto|always|never] [--artisan auto|always|never]
      [--package-install auto|always|never] [--build auto|always|never]
      [--install-command CMD] [--build-command CMD]
      [--pre-update-command CMD] [--post-update-command CMD]
      [--stop-before-pull SERVICE] [--stop-before-build SERVICE]
      [--restart-after SERVICE] [--start-after SERVICE]
      [--wait-timeout DETIK] [--no-queue-restart]
      [--allow-ip IP|CIDR] [--disabled]
  sm-updater app edit ID [opsi yang sama]
  sm-updater app list [--config PATH]
  sm-updater app show ID [--config PATH]
  sm-updater app remove ID [--config PATH]
  sm-updater app test ID [--config PATH]

Trigger:
  sm-updater run --all [--check]      Proses semua app aktif
  sm-updater run --app ID [--check]   Proses satu app
  sm-updater authorize [--app ID]     Validasi IP client (wrapper)
  sm-updater status                   Status update terakhir
  sm-updater logs [ID] [--tail N]     Log updater/project

  sm-updater --config PATH            Path projects.json
  sm-updater credential store         Simpan PAT Git (DPAPI/Credential Manager)
      --host HOST --username USER --project PATH [--git PATH]

Environment:
  SERVICE_MANAGER_HOME                Folder config default
  SERVICE_MANAGER_GIT                 Path git.exe untuk credential helper
  GCM_CREDENTIAL_STORE=dpapi          (diset otomatis)
  GCM_INTERACTIVE=never               (diset otomatis)
  GIT_TERMINAL_PROMPT=0               (diset otomatis)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn default_mode_is_menu() {
        let parsed = parse_args(&[]).unwrap();
        assert_eq!(parsed.mode, Mode::Menu);
        assert!(!parsed.all);
    }

    #[test]
    fn parses_commands_and_flags() {
        let parsed = parse_args(&args(&[
            "update",
            "--project",
            "kasirin",
            "--config",
            r"C:\config\projects.json",
            "--all",
        ]))
        .unwrap();
        assert_eq!(parsed.mode, Mode::Update);
        assert_eq!(parsed.project.as_deref(), Some("kasirin"));
        assert_eq!(
            parsed.config,
            Some(PathBuf::from(r"C:\config\projects.json"))
        );
        assert!(parsed.all);
    }

    #[test]
    fn parses_check_mode() {
        let parsed = parse_args(&args(&["check"])).unwrap();
        assert_eq!(parsed.mode, Mode::Check);
    }

    #[test]
    fn rejects_unknown_argument() {
        let error = parse_args(&args(&["deploy"])).unwrap_err();
        assert!(error.contains("tidak dikenal"));
    }

    #[test]
    fn rejects_missing_option_value() {
        assert!(parse_args(&args(&["--config"])).is_err());
        assert!(parse_args(&args(&["--project"])).is_err());
    }

    #[test]
    fn help_flag_is_recognized() {
        assert!(parse_args(&args(&["--help"])).unwrap().help);
    }

    #[test]
    fn parses_credential_store_defaults() {
        let parsed = parse_credential_args(&args(&["store"])).unwrap();
        assert_eq!(parsed.host, "gitlab.com");
        assert_eq!(parsed.username, "oauth2");
        assert!(!parsed.help);
    }

    #[test]
    fn parses_credential_flags() {
        let parsed = parse_credential_args(&args(&[
            "store",
            "--host",
            "github.com",
            "--username",
            "junioriqfar",
            "--project",
            r"C:\apps\demo",
            "--git",
            r"C:\git\git.exe",
        ]))
        .unwrap();
        assert_eq!(parsed.host, "github.com");
        assert_eq!(parsed.username, "junioriqfar");
        assert_eq!(parsed.project, PathBuf::from(r"C:\apps\demo"));
        assert_eq!(parsed.git, Some(PathBuf::from(r"C:\git\git.exe")));
    }

    #[test]
    fn credential_requires_store_subcommand() {
        assert!(parse_credential_args(&args(&["--host", "github.com"])).is_err());
        assert!(parse_credential_args(&args(&["store", "--host"])).is_err());
    }

    #[test]
    fn credential_missing_git_fails_before_prompt() {
        let temp = tempfile::tempdir().unwrap();
        let code = run_credential(&args(&[
            "store",
            "--git",
            r"C:\definitely-missing\git.exe",
            "--project",
            temp.path().to_str().unwrap(),
        ]));
        assert_eq!(code, 1);
    }

    #[test]
    fn default_log_dir_follows_config_parent() {
        let settings = ProjectsFile::default();
        let dir = log_dir(&settings, std::path::Path::new(r"C:\app\projects.json"));
        assert_eq!(dir, PathBuf::from(r"C:\app\logs"));
    }

    #[test]
    fn apply_settings_overrides_legacy_paths_and_log_dir() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        let mut config = UpdaterConfig::default();
        let settings_path = crate::settings::settings_path_for(&config_path);

        let mut settings = crate::settings::Settings {
            tools: crate::settings::Tools {
                git: crate::settings::ToolPath::new(r"C:\settings\git.exe"),
                ..crate::settings::Tools::default()
            },
            ..crate::settings::Settings::default()
        };
        settings.updater.log_dir = r"C:\settings\logs".into();
        crate::settings::save_settings(&settings_path, &settings).unwrap();

        apply_settings(&config_path, &mut config).unwrap();
        assert_eq!(config.settings.git_path, r"C:\settings\git.exe");
        assert_eq!(config.settings.log_dir, r"C:\settings\logs");
    }

    #[test]
    fn apply_settings_keeps_legacy_values_when_file_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        let mut config = UpdaterConfig {
            settings: crate::registry::UpdaterSettings {
                git_path: r"C:\legacy\git.exe".into(),
                ..crate::registry::UpdaterSettings::default()
            },
            ..UpdaterConfig::default()
        };

        apply_settings(&config_path, &mut config).unwrap();
        assert_eq!(config.settings.git_path, r"C:\legacy\git.exe");
        assert!(!crate::settings::settings_path_for(&config_path).exists());
    }
}
