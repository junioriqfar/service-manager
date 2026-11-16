pub mod app_update;
pub mod cli;
pub mod credential;
pub mod ip;
pub mod lock;
pub mod log;
pub mod registry;
pub mod runner;
pub mod scan;
pub mod services;
pub mod settings;
pub mod state;
pub mod sync;
pub mod toolchain;
pub mod trigger;
pub mod update;

pub use log::UpdateLogger;
pub use registry::{AppConfig, PackageManager, Preset, UpdaterConfig};
pub use runner::{Output, Runner, RunnerError, SystemRunner};
pub use scan::{discover_projects, is_laravel_project, ProjectInfo};
pub use settings::{
    effective_settings, effective_trigger, from_legacy, load_default_settings, load_settings,
    save_default_settings, save_settings, settings_path, settings_path_for, trigger_from_settings,
    ApacheToolPath, PrivilegedExecution, Settings, SettingsError, SettingsTrigger, ToolPath, Tools,
    UpdateTimeouts, UpdaterDefaults, SETTINGS_VERSION,
};
pub use update::{update_project, UpdateOptions, UpdateOutcome, UpdateStatus};
