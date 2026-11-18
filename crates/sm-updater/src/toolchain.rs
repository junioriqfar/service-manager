use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sm_core::ToggleMode;

use crate::registry::{AppConfig, CommandSpec, PackageManager, Preset, UpdaterSettings};
use crate::sync::{shell_command, TOOL_TIMEOUT};
pub use crate::variables::expand_variables;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCommand {
    pub label: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub env: BTreeMap<String, String>,
}

impl PlannedCommand {
    pub fn description(&self) -> String {
        let mut text = self.program.to_string_lossy().to_string();
        for arg in &self.args {
            text.push(' ');
            if arg.contains(' ') {
                text.push('"');
                text.push_str(arg);
                text.push('"');
            } else {
                text.push_str(arg);
            }
        }
        text
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolchainPlan {
    pub install: Vec<PlannedCommand>,
    pub build: Vec<PlannedCommand>,
    pub artisan: Vec<PlannedCommand>,
    pub notes: Vec<String>,
}

impl ToolchainPlan {
    pub fn all_commands(&self) -> impl Iterator<Item = &PlannedCommand> {
        self.install
            .iter()
            .chain(self.build.iter())
            .chain(self.artisan.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.install.is_empty() && self.build.is_empty() && self.artisan.is_empty()
    }
}

/// Tentukan preset dari isi folder project. Lockfile ganda tidak ditebak.
pub fn detect_preset(project: &Path) -> Preset {
    if project.join("go.mod").is_file() {
        Preset::Go
    } else if project.join("artisan").is_file() {
        Preset::Laravel
    } else if project.join("package.json").is_file() {
        if has_bun_lock(project) {
            Preset::Bun
        } else {
            Preset::Node
        }
    } else {
        Preset::Static
    }
}

pub fn detect_package_manager(project: &Path) -> Result<Option<PackageManager>, String> {
    let mut found = Vec::new();
    if project.join("package-lock.json").is_file() {
        found.push(PackageManager::Npm);
    }
    if project.join("pnpm-lock.yaml").is_file() {
        found.push(PackageManager::Pnpm);
    }
    if project.join("yarn.lock").is_file() {
        found.push(PackageManager::Yarn);
    }
    if has_bun_lock(project) {
        found.push(PackageManager::Bun);
    }

    match found.len() {
        0 => Ok(None),
        1 => Ok(found.into_iter().next()),
        _ => Err(format!(
            "Ambiguous package-manager lockfiles ({}). Set packageManager explicitly in the registry.",
            found
                .iter()
                .map(|value| match value {
                    PackageManager::Npm => "npm",
                    PackageManager::Pnpm => "pnpm",
                    PackageManager::Yarn => "yarn",
                    PackageManager::Bun => "bun",
                    _ => "unknown",
                })
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn has_bun_lock(project: &Path) -> bool {
    project.join("bun.lock").is_file() || project.join("bun.lockb").is_file()
}

/// Susun rencana toolchain sebelum file project diubah.
pub fn resolve_plan(
    app: &AppConfig,
    project: &Path,
    changed: &[String],
    settings: &UpdaterSettings,
) -> Result<ToolchainPlan, String> {
    if !project.is_dir() {
        return Err(format!("Project folder not found: {}", project.display()));
    }

    let preset = match app.preset.normalized() {
        Preset::Auto => detect_preset(project),
        other => other,
    };
    validate_preset(preset, project, app)?;

    let package_manager = resolve_package_manager(app, preset, project)?;
    let build_script = read_build_script(project)?;
    let mut plan = ToolchainPlan::default();

    if app.composer.normalized() == ToggleMode::Always && !project.join("composer.json").is_file() {
        return Err("composer=always but composer.json was not found.".to_string());
    }

    if composer_should_install(app, project, changed) {
        plan.install.push(composer_command(settings, project)?);
    }

    let install_override = app.install_command.trim();
    let install_requested =
        package_install_requested(app, preset, package_manager, project, changed);
    if !install_override.is_empty() {
        if install_requested {
            plan.install.push(shell_planned(
                "install command",
                &expand_variables(install_override, &settings.variables),
                project,
            ));
        }
    } else if preset == Preset::Custom {
        if install_requested {
            append_specs(
                &mut plan.install,
                &app.commands.install,
                project,
                &settings.variables,
            )?;
        }
    } else if let Some(manager) = package_manager {
        if install_requested {
            plan.install
                .push(package_install_command(settings, manager, project)?);
        }
    } else if preset == Preset::Go && install_requested {
        plan.install.push(go_install_command(settings, project)?);
    }

    let build_requested = build_requested(app, preset, build_script.as_deref(), changed);
    let build_override = app.build_command.trim();
    if preset == Preset::Custom {
        if app.build.normalized() != ToggleMode::Never {
            append_specs(
                &mut plan.build,
                &app.commands.build,
                project,
                &settings.variables,
            )?;
            if !build_override.is_empty() {
                plan.build.push(shell_planned(
                    "build command",
                    &expand_variables(build_override, &settings.variables),
                    project,
                ));
            }
        }
    } else if build_requested {
        if !build_override.is_empty() {
            plan.build.push(shell_planned(
                "build command",
                &expand_variables(build_override, &settings.variables),
                project,
            ));
        } else if preset == Preset::Go {
            if app.build.normalized() == ToggleMode::Always {
                return Err(
                    "the go preset requires buildCommand (for example 'make build-windows' or 'go build -o build/app.exe .')."
                        .to_string(),
                );
            }
            plan.notes.push(
                "the go preset has no buildCommand; the build is skipped (set buildCommand for automatic builds)."
                    .to_string(),
            );
        } else if let Some(manager) = package_manager {
            if build_script.is_none() {
                return Err(
                    "Build requested but package.json has no scripts.build; set buildCommand."
                        .to_string(),
                );
            }
            plan.build
                .push(package_build_command(settings, manager, project)?);
        } else {
            return Err(
                "Build requested but no package manager or buildCommand is available.".to_string(),
            );
        }
        append_specs(
            &mut plan.build,
            &app.commands.build,
            project,
            &settings.variables,
        )?;
    }

    if preset == Preset::Laravel {
        plan.artisan = artisan_commands(app, project, changed, settings)?;
    }

    if package_manager.is_none()
        && project.join("package.json").is_file()
        && preset != Preset::Custom
    {
        plan.notes.push(
            "package.json found but no package manager is selected; Node dependencies are skipped."
                .to_string(),
        );
    }

    Ok(plan)
}

fn validate_preset(preset: Preset, project: &Path, app: &AppConfig) -> Result<(), String> {
    match preset {
        Preset::Laravel => {
            if !project.join("artisan").is_file() || !project.join("composer.json").is_file() {
                return Err(
                    "the laravel preset requires artisan and composer.json in the project folder."
                        .to_string(),
                );
            }
        }
        Preset::Node | Preset::Bun => {
            if !project.join("package.json").is_file() {
                return Err(format!(
                    "the {} preset requires package.json.",
                    if preset == Preset::Bun { "bun" } else { "node" }
                ));
            }
        }
        Preset::Go => {
            if !project.join("go.mod").is_file() {
                return Err("the go preset requires go.mod.".to_string());
            }
        }
        Preset::Custom => {
            let has_commands = !app.commands.install.is_empty()
                || !app.commands.build.is_empty()
                || !app.commands.pre_update.is_empty()
                || !app.commands.post_update.is_empty();
            if !has_commands
                && app.install_command.trim().is_empty()
                && app.build_command.trim().is_empty()
            {
                return Err(
                    "the custom preset requires commands.* or installCommand/buildCommand."
                        .to_string(),
                );
            }
        }
        Preset::Auto | Preset::Static => {}
        Preset::Unknown => {
            return Err("Unknown preset; use auto/laravel/node/bun/go/static/custom.".to_string())
        }
    }
    Ok(())
}

fn resolve_package_manager(
    app: &AppConfig,
    preset: Preset,
    project: &Path,
) -> Result<Option<PackageManager>, String> {
    let has_package_json = project.join("package.json").is_file();
    let explicit = app.package_manager.normalized();
    let detected = if explicit == PackageManager::Auto {
        detect_package_manager(project)?
    } else {
        None
    };

    if !has_package_json {
        if matches!(
            explicit,
            PackageManager::Npm | PackageManager::Pnpm | PackageManager::Yarn | PackageManager::Bun
        ) {
            return Err(format!(
                "packageManager={} but package.json was not found.",
                package_manager_name(explicit)
            ));
        }
        return Ok(None);
    }

    match explicit {
        PackageManager::Auto => match preset {
            Preset::Bun => Ok(Some(PackageManager::Bun)),
            Preset::Node | Preset::Laravel => Ok(Some(detected.unwrap_or(PackageManager::Npm))),
            _ => Ok(None),
        },
        PackageManager::None => Ok(None),
        other => Ok(Some(other)),
    }
}

fn package_manager_name(manager: PackageManager) -> &'static str {
    match manager {
        PackageManager::Npm => "npm",
        PackageManager::Pnpm => "pnpm",
        PackageManager::Yarn => "yarn",
        PackageManager::Bun => "bun",
        PackageManager::None => "none",
        PackageManager::Auto => "auto",
        PackageManager::Unknown => "unknown",
    }
}

fn composer_should_install(app: &AppConfig, project: &Path, changed: &[String]) -> bool {
    match app.composer.normalized() {
        ToggleMode::Always => true,
        ToggleMode::Never | ToggleMode::Unknown => false,
        ToggleMode::Auto => {
            project.join("composer.json").is_file()
                && (changed.iter().any(|file| {
                    let file = normalize_path(file);
                    matches!(file.as_str(), "composer.json" | "composer.lock")
                }) || !project.join("vendor").is_dir())
        }
    }
}

fn package_install_requested(
    app: &AppConfig,
    preset: Preset,
    manager: Option<PackageManager>,
    project: &Path,
    changed: &[String],
) -> bool {
    match app.package_install.normalized() {
        ToggleMode::Always => true,
        ToggleMode::Never | ToggleMode::Unknown => false,
        ToggleMode::Auto => {
            if preset == Preset::Custom || !app.install_command.trim().is_empty() {
                return true;
            }
            if preset == Preset::Go {
                return true;
            }
            if manager.is_none() {
                return false;
            }
            if !project.join("node_modules").is_dir() {
                return true;
            }
            changed.iter().any(|file| {
                matches!(
                    normalize_path(file).as_str(),
                    "package.json"
                        | "package-lock.json"
                        | "pnpm-lock.yaml"
                        | "yarn.lock"
                        | "bun.lock"
                        | "bun.lockb"
                )
            })
        }
    }
}

fn build_requested(
    app: &AppConfig,
    preset: Preset,
    build_script: Option<&str>,
    changed: &[String],
) -> bool {
    match app.build.normalized() {
        ToggleMode::Always => true,
        ToggleMode::Never | ToggleMode::Unknown => false,
        ToggleMode::Auto => {
            if !app.build_command.trim().is_empty() {
                return match preset {
                    Preset::Go | Preset::Custom | Preset::Static => !changed.is_empty(),
                    _ => frontend_changed(changed),
                };
            }
            match preset {
                Preset::Custom => true,
                Preset::Go => !changed.is_empty(),
                _ => build_script.is_some() && frontend_changed(changed),
            }
        }
    }
}

fn frontend_changed(changed: &[String]) -> bool {
    changed.iter().any(|file| {
        let path = normalize_path(file);
        path.starts_with("resources/")
            || path.starts_with("src/")
            || path.starts_with("public/")
            || path.starts_with("assets/")
            || matches!(
                path.as_str(),
                "package.json"
                    | "package-lock.json"
                    | "pnpm-lock.yaml"
                    | "yarn.lock"
                    | "bun.lock"
                    | "bun.lockb"
                    | "vite.config.ts"
                    | "vite.config.js"
                    | "webpack.config.js"
                    | "webpack.config.ts"
                    | "tsconfig.json"
                    | "tailwind.config.js"
                    | "tailwind.config.ts"
                    | "postcss.config.js"
                    | "postcss.config.cjs"
            )
            || [
                ".ts", ".tsx", ".js", ".jsx", ".css", ".scss", ".sass", ".vue", ".svelte",
            ]
            .iter()
            .any(|extension| path.ends_with(extension))
    })
}

fn normalize_path(value: &str) -> String {
    value.trim().replace('\\', "/")
}

fn read_build_script(project: &Path) -> Result<Option<String>, String> {
    let path = project.join("package.json");
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read package.json: {error}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|error| format!("package.json is invalid: {error}"))?;
    Ok(value
        .get("scripts")
        .and_then(|scripts| scripts.get("build"))
        .and_then(|script| script.as_str())
        .map(|script| script.to_string()))
}

fn composer_command(settings: &UpdaterSettings, project: &Path) -> Result<PlannedCommand, String> {
    Ok(PlannedCommand {
        label: "composer install".to_string(),
        program: require_tool(&settings.composer_path, "Composer")?,
        args: [
            "install",
            "--no-dev",
            "--optimize-autoloader",
            "--no-interaction",
        ]
        .iter()
        .map(|value| value.to_string())
        .collect(),
        cwd: project.to_path_buf(),
        timeout: TOOL_TIMEOUT,
        env: BTreeMap::new(),
    })
}

fn package_install_command(
    settings: &UpdaterSettings,
    manager: PackageManager,
    project: &Path,
) -> Result<PlannedCommand, String> {
    let program = package_manager_tool(settings, manager)?;
    let args = match manager {
        PackageManager::Npm => {
            if project.join("package-lock.json").is_file() {
                vec!["ci".to_string()]
            } else {
                vec!["install".to_string()]
            }
        }
        PackageManager::Pnpm => {
            let mut args = vec!["install".to_string()];
            if project.join("pnpm-lock.yaml").is_file() {
                args.push("--frozen-lockfile".to_string());
            }
            args
        }
        PackageManager::Yarn => {
            let mut args = vec!["install".to_string()];
            if project.join("yarn.lock").is_file() {
                args.push("--frozen-lockfile".to_string());
            }
            args
        }
        PackageManager::Bun => {
            let mut args = vec!["install".to_string()];
            if has_bun_lock(project) {
                args.push("--frozen-lockfile".to_string());
            }
            args
        }
        _ => Vec::new(),
    };
    Ok(PlannedCommand {
        label: format!("{} install", package_manager_name(manager)),
        program,
        args,
        cwd: project.to_path_buf(),
        timeout: TOOL_TIMEOUT,
        env: BTreeMap::new(),
    })
}

fn package_build_command(
    settings: &UpdaterSettings,
    manager: PackageManager,
    project: &Path,
) -> Result<PlannedCommand, String> {
    Ok(PlannedCommand {
        label: format!("{} build", package_manager_name(manager)),
        program: package_manager_tool(settings, manager)?,
        args: vec!["run".to_string(), "build".to_string()],
        cwd: project.to_path_buf(),
        timeout: TOOL_TIMEOUT,
        env: BTreeMap::new(),
    })
}

fn package_manager_tool(
    settings: &UpdaterSettings,
    manager: PackageManager,
) -> Result<PathBuf, String> {
    match manager {
        PackageManager::Npm => require_tool(&settings.npm_path, "npm"),
        PackageManager::Pnpm => require_tool(&settings.pnpm_path, "pnpm"),
        PackageManager::Yarn => require_tool(&settings.yarn_path, "yarn"),
        PackageManager::Bun => require_tool(&settings.bun_path, "Bun"),
        _ => Err(format!(
            "Package manager {} is not supported for install/build.",
            package_manager_name(manager)
        )),
    }
}

fn go_install_command(
    settings: &UpdaterSettings,
    project: &Path,
) -> Result<PlannedCommand, String> {
    Ok(PlannedCommand {
        label: "go mod download".to_string(),
        program: require_tool(&settings.go_path, "Go")?,
        args: vec!["mod".to_string(), "download".to_string()],
        cwd: project.to_path_buf(),
        timeout: TOOL_TIMEOUT,
        env: BTreeMap::new(),
    })
}

fn artisan_commands(
    app: &AppConfig,
    project: &Path,
    changed: &[String],
    settings: &UpdaterSettings,
) -> Result<Vec<PlannedCommand>, String> {
    let mode = app.artisan.normalized();
    if mode == ToggleMode::Never || mode == ToggleMode::Unknown {
        return Ok(Vec::new());
    }
    let cache_files_exist = [
        "bootstrap/cache/config.php",
        "bootstrap/cache/routes.php",
        "bootstrap/cache/routes-v7.php",
    ]
    .iter()
    .any(|file| project.join(file).is_file());
    let code_changed = changed
        .iter()
        .any(|file| sm_core::update::is_code_change(file));

    let optimize_clear = match mode {
        ToggleMode::Always => true,
        ToggleMode::Auto => cache_files_exist,
        _ => false,
    };
    let queue_restart = app.queue_restart
        && match mode {
            ToggleMode::Always => true,
            ToggleMode::Auto => code_changed,
            _ => false,
        };

    if !optimize_clear && !queue_restart {
        return Ok(Vec::new());
    }
    let php = require_tool(&settings.php_path, "PHP")?;
    let mut commands = Vec::new();
    if optimize_clear {
        commands.push(PlannedCommand {
            label: "artisan optimize:clear".to_string(),
            program: php.clone(),
            args: vec!["artisan".to_string(), "optimize:clear".to_string()],
            cwd: project.to_path_buf(),
            timeout: TOOL_TIMEOUT,
            env: BTreeMap::new(),
        });
    }
    if queue_restart {
        commands.push(PlannedCommand {
            label: "artisan queue:restart".to_string(),
            program: php,
            args: vec!["artisan".to_string(), "queue:restart".to_string()],
            cwd: project.to_path_buf(),
            timeout: TOOL_TIMEOUT,
            env: BTreeMap::new(),
        });
    }
    Ok(commands)
}

fn shell_planned(label: &str, line: &str, cwd: &Path) -> PlannedCommand {
    let (program, args) = shell_command(line);
    PlannedCommand {
        label: label.to_string(),
        program,
        args,
        cwd: cwd.to_path_buf(),
        timeout: TOOL_TIMEOUT,
        env: BTreeMap::new(),
    }
}

pub(crate) fn append_specs(
    target: &mut Vec<PlannedCommand>,
    specs: &[CommandSpec],
    project: &Path,
    variables: &BTreeMap<String, String>,
) -> Result<(), String> {
    for spec in specs {
        target.push(planned_from_spec(spec, project, variables)?);
    }
    Ok(())
}

pub(crate) fn planned_from_spec(
    spec: &CommandSpec,
    project: &Path,
    variables: &BTreeMap<String, String>,
) -> Result<PlannedCommand, String> {
    let cwd_text = expand_variables(&spec.cwd, variables);
    let cwd = resolve_cwd(&cwd_text, project)?;
    let shell = expand_variables(&spec.shell, variables);
    let shell = shell.trim();
    if !shell.is_empty() {
        let (program, args) = shell_command(shell);
        return Ok(PlannedCommand {
            label: if spec.name.trim().is_empty() {
                "custom shell".to_string()
            } else {
                spec.name.trim().to_string()
            },
            program,
            args,
            cwd,
            timeout: Duration::from_secs(spec.timeout_seconds.max(1)),
            env: expanded_env(&spec.env, variables),
        });
    }
    let program_text_owned = expand_variables(&spec.program, variables);
    let program_text = program_text_owned.trim();
    if program_text.is_empty() {
        return Err(format!(
            "Command '{}' has no program.",
            if spec.name.trim().is_empty() {
                "(unnamed)"
            } else {
                spec.name.trim()
            }
        ));
    }
    let program = PathBuf::from(program_text);
    if (program_text.contains('\\') || program_text.contains('/')) && !program.is_file() {
        return Err(format!("Command program not found: {}", program.display()));
    }
    Ok(PlannedCommand {
        label: if spec.name.trim().is_empty() {
            program_text.to_string()
        } else {
            spec.name.trim().to_string()
        },
        program,
        args: spec
            .args
            .iter()
            .map(|arg| expand_variables(arg, variables))
            .collect(),
        cwd,
        timeout: Duration::from_secs(spec.timeout_seconds.max(1)),
        env: expanded_env(&spec.env, variables),
    })
}

fn expanded_env(
    env: &BTreeMap<String, String>,
    variables: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    env.iter()
        .map(|(key, value)| (key.clone(), expand_variables(value, variables)))
        .collect()
}

fn resolve_cwd(value: &str, project: &Path) -> Result<PathBuf, String> {
    let value = value.trim();
    let cwd = if value.is_empty() {
        project.to_path_buf()
    } else {
        let path = Path::new(value);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            project.join(path)
        }
    };
    if !cwd.is_dir() {
        return Err(format!(
            "Command working directory not found: {}",
            cwd.display()
        ));
    }
    Ok(cwd)
}

fn require_tool(path: &str, label: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    if !path.is_empty() {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }
    if let Some(found) = find_on_path(tool_names(label)) {
        return Ok(found);
    }
    if path.is_empty() {
        Err(format!(
            "{label} was not found in settings or PATH. Set its path in data/projects.json."
        ))
    } else {
        Err(format!("{label} not found: {path}"))
    }
}

fn tool_names(label: &str) -> &'static [&'static str] {
    match label {
        "Composer" => &["composer.bat", "composer.cmd", "composer.exe"],
        "npm" => &["npm.cmd", "npm.exe"],
        "pnpm" => &["pnpm.cmd", "pnpm.exe"],
        "yarn" => &["yarn.cmd", "yarn.exe"],
        "Bun" => &["bun.exe"],
        "Go" => &["go.exe"],
        "PHP" => &["php.exe"],
        _ => &[],
    }
}

fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        for name in names {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{AppConfig, UpdaterSettings};

    struct Fixture {
        _temp: tempfile::TempDir,
        project: PathBuf,
        settings: UpdaterSettings,
    }

    fn fixture(files: &[(&str, &str)]) -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        for (name, content) in files {
            let path = project.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        for name in [
            "git.exe",
            "php.exe",
            "composer.bat",
            "npm.cmd",
            "bun.exe",
            "go.exe",
        ] {
            std::fs::write(tools.join(name), b"dummy").unwrap();
        }
        let mut settings = UpdaterSettings {
            git_path: tools.join("git.exe").to_string_lossy().to_string(),
            php_path: tools.join("php.exe").to_string_lossy().to_string(),
            composer_path: tools.join("composer.bat").to_string_lossy().to_string(),
            npm_path: tools.join("npm.cmd").to_string_lossy().to_string(),
            bun_path: tools.join("bun.exe").to_string_lossy().to_string(),
            go_path: tools.join("go.exe").to_string_lossy().to_string(),
            ..UpdaterSettings::default()
        };
        settings.refresh_variables();
        Fixture {
            _temp: temp,
            project,
            settings,
        }
    }

    fn app(preset: Preset) -> AppConfig {
        AppConfig {
            id: "demo".into(),
            repo_url: "https://example.com/demo.git".into(),
            local_path: "demo".into(),
            preset,
            ..AppConfig::default()
        }
    }

    #[test]
    fn detects_laravel_npm_and_plans_install_build_artisan() {
        let fixture = fixture(&[
            ("artisan", "php"),
            ("composer.json", "{}"),
            ("composer.lock", "{}"),
            ("package.json", r#"{"scripts":{"build":"vite build"}}"#),
            ("package-lock.json", "{}"),
            ("bootstrap/cache/config.php", "cache"),
        ]);
        let mut app = app(Preset::Laravel);
        app.composer = ToggleMode::Always;
        app.package_install = ToggleMode::Always;
        app.build = ToggleMode::Always;

        let plan = resolve_plan(
            &app,
            &fixture.project,
            &["composer.lock".into(), "resources/js/app.ts".into()],
            &fixture.settings,
        )
        .unwrap();

        assert_eq!(plan.install.len(), 2);
        assert_eq!(plan.install[0].label, "composer install");
        assert_eq!(plan.install[1].args, ["ci"]);
        assert_eq!(plan.build[0].args, ["run", "build"]);
        assert!(plan
            .artisan
            .iter()
            .any(|cmd| cmd.label.contains("optimize")));
        assert!(plan.artisan.iter().any(|cmd| cmd.label.contains("queue")));
    }

    #[test]
    fn ambiguous_lockfiles_fail() {
        let fixture = fixture(&[
            ("package.json", r#"{"scripts":{"build":"vite build"}}"#),
            ("package-lock.json", "{}"),
            ("yarn.lock", "{}"),
        ]);
        let app = app(Preset::Node);
        let error = resolve_plan(
            &app,
            &fixture.project,
            &["src/app.ts".into()],
            &fixture.settings,
        )
        .unwrap_err();
        assert!(error.contains("Ambiguous"));
    }

    #[test]
    fn bun_uses_bun_lock_and_build_script() {
        let fixture = fixture(&[
            ("package.json", r#"{"scripts":{"build":"bun build"}}"#),
            ("bun.lock", "{}"),
        ]);
        let mut app = app(Preset::Bun);
        app.package_install = ToggleMode::Always;
        app.build = ToggleMode::Always;
        let plan = resolve_plan(
            &app,
            &fixture.project,
            &["src/main.ts".into()],
            &fixture.settings,
        )
        .unwrap();
        assert!(plan.install[0].program.ends_with("bun.exe"));
        assert_eq!(plan.install[0].args, ["install", "--frozen-lockfile"]);
        assert_eq!(plan.build[0].args, ["run", "build"]);
    }

    #[test]
    fn node_without_build_script_only_installs() {
        let fixture = fixture(&[
            ("package.json", r#"{"scripts":{}}"#),
            ("package-lock.json", "{}"),
        ]);
        let mut app = app(Preset::Node);
        app.package_install = ToggleMode::Always;
        app.build = ToggleMode::Auto;
        let plan = resolve_plan(
            &app,
            &fixture.project,
            &["src/app.js".into()],
            &fixture.settings,
        )
        .unwrap();
        assert_eq!(plan.install.len(), 1);
        assert!(plan.build.is_empty());
    }

    #[test]
    fn explicit_build_command_runs_without_build_script() {
        let fixture = fixture(&[("package.json", r#"{"scripts":{}}"#), ("bun.lock", "{}")]);
        let mut app = app(Preset::Bun);
        app.package_install = ToggleMode::Always;
        app.build = ToggleMode::Auto;
        app.build_command = "bun run build:win".into();
        let plan = resolve_plan(
            &app,
            &fixture.project,
            &["src/main.ts".into()],
            &fixture.settings,
        )
        .unwrap();
        assert_eq!(plan.build.len(), 1);
        assert_eq!(plan.build[0].args.last().unwrap(), "bun run build:win");
    }

    #[test]
    fn go_requires_build_command_but_installs_modules() {
        let fixture = fixture(&[("go.mod", "module demo")]);
        let mut app = app(Preset::Go);
        app.build = ToggleMode::Auto;
        let plan = resolve_plan(
            &app,
            &fixture.project,
            &["main.go".into()],
            &fixture.settings,
        )
        .unwrap();
        assert_eq!(plan.install[0].label, "go mod download");
        assert!(plan.build.is_empty());

        app.build = ToggleMode::Always;
        let error = resolve_plan(
            &app,
            &fixture.project,
            &["main.go".into()],
            &fixture.settings,
        )
        .unwrap_err();
        assert!(error.contains("buildCommand"));
    }

    #[test]
    fn go_build_uses_explicit_shell_command() {
        let fixture = fixture(&[("go.mod", "module demo")]);
        let mut app = app(Preset::Go);
        app.build = ToggleMode::Always;
        app.build_command = "make build-windows".into();
        let plan = resolve_plan(
            &app,
            &fixture.project,
            &["main.go".into()],
            &fixture.settings,
        )
        .unwrap();
        assert_eq!(plan.build[0].args.last().unwrap(), "make build-windows");
    }

    #[test]
    fn static_project_has_no_commands() {
        let fixture = fixture(&[("index.html", "hello")]);
        let app = app(Preset::Static);
        let plan = resolve_plan(
            &app,
            &fixture.project,
            &["index.html".into()],
            &fixture.settings,
        )
        .unwrap();
        assert!(plan.is_empty());
    }

    #[test]
    fn custom_commands_are_validated() {
        let fixture = fixture(&[]);
        let app = app(Preset::Custom);
        let error = resolve_plan(&app, &fixture.project, &[], &fixture.settings).unwrap_err();
        assert!(error.contains("custom"));
    }

    #[test]
    fn composer_auto_installs_when_vendor_missing() {
        let fixture = fixture(&[("composer.json", "{}"), ("composer.lock", "{}")]);
        let mut app = app(Preset::Laravel);
        app.composer = ToggleMode::Auto;
        // artisan/composer validation is bypassed by using a custom preset with composer.
        app.preset = Preset::Static;
        let plan = resolve_plan(&app, &fixture.project, &[], &fixture.settings).unwrap();
        assert_eq!(plan.install.len(), 1);
        assert_eq!(plan.install[0].label, "composer install");
    }

    #[test]
    fn custom_command_specs_expand_variables() {
        let fixture = fixture(&[]);
        let php = fixture.settings.variables["PHP"].clone();
        let php_home = fixture.settings.variables["PHP_HOME"].clone();
        let spec = CommandSpec {
            name: "php artisan".into(),
            program: "${PHP}".into(),
            args: vec![
                "${PHP_HOME}\\artisan".into(),
                "migrate".into(),
                "--force".into(),
            ],
            cwd: "${PHP_HOME}".into(),
            env: BTreeMap::from([("SM_PHP_HOME".into(), "${PHP_HOME}".into())]),
            ..CommandSpec::default()
        };

        let command = planned_from_spec(&spec, &fixture.project, &fixture.settings.variables)
            .expect("custom command");

        assert_eq!(command.program, PathBuf::from(&php));
        assert_eq!(command.args[0], format!(r"{php_home}\artisan"));
        assert_eq!(command.cwd, PathBuf::from(&php_home));
        assert_eq!(command.env["SM_PHP_HOME"], php_home);
    }

    #[test]
    fn custom_shell_spec_expands_variables() {
        let fixture = fixture(&[]);
        let php = fixture.settings.variables["PHP"].clone();
        let php_home = fixture.settings.variables["PHP_HOME"].clone();
        let spec = CommandSpec {
            name: "shell".into(),
            shell: r#""${PHP}" "${PHP_HOME}\artisan" migrate"#.into(),
            cwd: "${PHP_HOME}".into(),
            ..CommandSpec::default()
        };

        let command = planned_from_spec(&spec, &fixture.project, &fixture.settings.variables)
            .expect("custom shell command");

        assert_eq!(command.cwd, PathBuf::from(&php_home));
        assert_eq!(
            command.args.last().unwrap(),
            &format!(r#""{php}" "{php_home}\artisan" migrate"#)
        );
    }

    #[test]
    fn install_and_build_commands_expand_variables() {
        let fixture = fixture(&[]);
        let php = fixture.settings.variables["PHP"].clone();
        let php_home = fixture.settings.variables["PHP_HOME"].clone();
        let mut app = app(Preset::Custom);
        app.package_install = ToggleMode::Always;
        app.build = ToggleMode::Always;
        app.install_command = r#""${PHP}" artisan migrate"#.into();
        app.build_command = r#"echo ${PHP_HOME}"#.into();

        let plan = resolve_plan(&app, &fixture.project, &[], &fixture.settings).unwrap();

        assert_eq!(plan.install.len(), 1);
        assert_eq!(
            plan.install[0].args.last().unwrap(),
            &format!(r#""{php}" artisan migrate"#)
        );
        assert_eq!(plan.build.len(), 1);
        assert_eq!(
            plan.build[0].args.last().unwrap(),
            &format!("echo {php_home}")
        );
    }
}
