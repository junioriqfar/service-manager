use std::io::{self, Write};
use std::path::PathBuf;

use sm_config::{load_projects, ConfigPaths, ProjectsFile};

use crate::credential::{store_credential, verify_remote, CredentialRequest, GitCredentialSink};
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
    if values.first().map(String::as_str) == Some("credential") {
        return run_credential(&values[1..]);
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
  sm-updater                          Menu interaktif
  sm-updater list                     Daftar project
  sm-updater check [--project NAMA]   Cek update (dry-run)
  sm-updater update [--project NAMA]  Update project
  sm-updater --all                    Proses semua project aktif
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
}
