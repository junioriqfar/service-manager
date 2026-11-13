use std::io::{self, Write};
use std::path::PathBuf;

use sm_config::{load_projects, ConfigPaths, ProjectsFile};

use crate::runner::SystemRunner;
use crate::scan::{discover_projects, ProjectInfo};
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

fn print_usage() {
    println!(
        "Service Manager Project Updater

Penggunaan:
  sm-updater                          Menu interaktif
  sm-updater list                     Daftar project
  sm-updater check [--project NAMA]   Cek update (dry-run)
  sm-updater update [--project NAMA]  Update project
  sm-updater --all                    Proses semua project aktif
  sm-updater --config PATH            Path projects.json

Environment:
  SERVICE_MANAGER_HOME                Folder config default
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
    fn default_log_dir_follows_config_parent() {
        let settings = ProjectsFile::default();
        let dir = log_dir(&settings, std::path::Path::new(r"C:\app\projects.json"));
        assert_eq!(dir, PathBuf::from(r"C:\app\logs"));
    }
}
