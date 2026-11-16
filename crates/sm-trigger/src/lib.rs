use std::env;
use std::process::Command;

pub const DEFAULT_REMOTE_BIN: &str = r"C:\tools\ServiceManager\target\release\sm-updater.exe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub identity: Option<String>,
    pub ssh_options: Vec<String>,
    pub remote_bin: String,
    pub wrapper: Option<String>,
    pub config: Option<String>,
    pub app: Option<String>,
    pub check: bool,
    pub dry_run: bool,
    pub help: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            host: String::new(),
            user: String::new(),
            port: 22,
            identity: None,
            ssh_options: Vec::new(),
            remote_bin: DEFAULT_REMOTE_BIN.to_string(),
            wrapper: None,
            config: None,
            app: None,
            check: false,
            dry_run: false,
            help: false,
        }
    }
}

pub fn parse_args(values: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut index = 0usize;
    while index < values.len() {
        match values[index].as_str() {
            "--help" | "-h" => args.help = true,
            "--host" => args.host = take_value(values, &mut index, "--host")?,
            "--user" => args.user = take_value(values, &mut index, "--user")?,
            "--port" => {
                let value = take_value(values, &mut index, "--port")?;
                args.port = value
                    .parse::<u16>()
                    .map_err(|_| format!("--port tidak valid: {value}"))?;
            }
            "--identity" | "-i" => {
                args.identity = Some(take_value(values, &mut index, "--identity")?)
            }
            "--ssh-option" | "-o" => {
                args.ssh_options
                    .push(take_value(values, &mut index, "--ssh-option")?)
            }
            "--remote-bin" => args.remote_bin = take_value(values, &mut index, "--remote-bin")?,
            "--wrapper" => args.wrapper = Some(take_value(values, &mut index, "--wrapper")?),
            "--config" => args.config = Some(take_value(values, &mut index, "--config")?),
            "--app" => args.app = Some(take_value(values, &mut index, "--app")?),
            "--check" => args.check = true,
            "--all" => args.app = None,
            "--dry-run" | "--print" => args.dry_run = true,
            other => return Err(format!("Argumen tidak dikenal: {other}")),
        }
        index += 1;
    }

    if args.help {
        return Ok(args);
    }
    if args.host.trim().is_empty() {
        return Err("--host wajib diisi.".to_string());
    }
    if args.user.trim().is_empty() {
        args.user = default_user().ok_or_else(|| {
            "--user wajib diisi (atau set environment USERNAME/USER).".to_string()
        })?;
    }
    if args.app.as_deref().map(str::trim).unwrap_or("").is_empty() {
        args.app = None;
    }
    Ok(args)
}

fn take_value(values: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    values
        .get(*index)
        .cloned()
        .ok_or_else(|| format!("{flag} membutuhkan nilai."))
}

pub fn default_user() -> Option<String> {
    for name in ["SM_TRIGGER_USER", "USERNAME", "USER"] {
        if let Ok(value) = env::var(name) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    None
}

pub fn ssh_args(args: &Args, remote_command: &str) -> Vec<String> {
    let mut values = Vec::new();
    if args.port != 22 {
        values.push("-p".to_string());
        values.push(args.port.to_string());
    }
    if let Some(identity) = &args.identity {
        values.push("-i".to_string());
        values.push(identity.clone());
    }
    for option in &args.ssh_options {
        values.push("-o".to_string());
        values.push(option.clone());
    }
    values.push(format!("{}@{}", args.user, args.host));
    values.push(remote_command.to_string());
    values
}

pub fn remote_command(args: &Args) -> String {
    if let Some(wrapper) = args
        .wrapper
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let mut parts = vec![
            "powershell".to_string(),
            "-NoProfile".to_string(),
            "-ExecutionPolicy".to_string(),
            "Bypass".to_string(),
            "-File".to_string(),
            quote_arg(wrapper.trim()),
        ];
        if let Some(app) = &args.app {
            parts.push("-App".to_string());
            parts.push(quote_arg(app));
        }
        if args.check {
            parts.push("-Check".to_string());
        }
        if let Some(config) = &args.config {
            parts.push("-Config".to_string());
            parts.push(quote_arg(config));
        }
        return parts.join(" ");
    }

    let mut parts = vec![quote_arg(&args.remote_bin), "run".to_string()];
    match &args.app {
        Some(app) => {
            parts.push("--app".to_string());
            parts.push(quote_arg(app));
        }
        None => parts.push("--all".to_string()),
    }
    if args.check {
        parts.push("--check".to_string());
    }
    if let Some(config) = &args.config {
        parts.push("--config".to_string());
        parts.push(quote_arg(config));
    }
    parts.join(" ")
}

pub fn quote_arg(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    let safe = value.chars().all(|character| {
        character.is_ascii_alphanumeric()
            || matches!(
                character,
                '-' | '_' | '.' | '/' | '\\' | ':' | '@' | '+' | '=' | '~'
            )
    });
    if safe {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('"', "\\\""))
    }
}

pub fn preview(args: &Args) -> String {
    preview_with(args, &remote_command(args))
}

pub fn preview_with(args: &Args, remote_command: &str) -> String {
    let ssh = ssh_args(args, remote_command);
    format!(
        "ssh {}",
        ssh.iter()
            .map(|value| quote_arg(value))
            .collect::<Vec<_>>()
            .join(" ")
    )
}

pub fn ssh_program_args(args: &Args) -> (String, Vec<String>) {
    ssh_program_args_with(args, &remote_command(args))
}

pub fn ssh_program_args_with(args: &Args, remote_command: &str) -> (String, Vec<String>) {
    ("ssh".to_string(), ssh_args(args, remote_command))
}

pub fn run(args: &Args) -> i32 {
    let command = remote_command(args);
    let ssh = ssh_args(args, &command);
    if args.dry_run {
        println!("{}", preview(args));
        return 0;
    }

    match Command::new("ssh").args(&ssh).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("Gagal menjalankan ssh: {error}");
            1
        }
    }
}

pub fn print_usage() {
    println!(
        "sm-trigger - SSH trigger untuk Service Manager updater

Penggunaan:
  sm-trigger --host HOST --user USER --all
  sm-trigger --host HOST --user USER --app ID
  sm-trigger --host HOST --user USER --all --check
  sm-trigger --host HOST --user USER --all --dry-run

Opsi:
  --host HOST             Host server Windows (wajib)
  --user USER             User SSH (default USERNAME/USER)
  --port PORT             Port SSH (default 22)
  --identity PATH         File private key SSH (-i)
  --ssh-option VALUE      Opsi ssh tambahan, boleh berulang (-o)
  --remote-bin PATH       Path sm-updater.exe di server
  --wrapper PATH          Script PowerShell wrapper/elevated task di server
  --config PATH           Path projects.json di server
  --app ID                Trigger satu app
  --all                   Trigger semua app (default)
  --check                 Dry-run di server
  --dry-run, --print      Hanya cetak perintah ssh
  --help                  Bantuan

Contoh:
  sm-trigger --host 10.255.240.160 --user EGOV22 --all
  sm-trigger --host 10.255.240.160 --user EGOV22 --app opencode-quota
  sm-trigger --host 10.255.240.160 --user EGOV22 --all `
    --wrapper C:\\tools\\ServiceManager\\scripts\\Invoke-UpdaterTask.ps1"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn base() -> Args {
        Args {
            host: "10.255.240.160".into(),
            user: "EGOV22".into(),
            ..Args::default()
        }
    }

    #[test]
    fn builds_all_command() {
        let command = remote_command(&base());
        assert_eq!(
            command,
            r"C:\tools\ServiceManager\target\release\sm-updater.exe run --all"
        );
    }

    #[test]
    fn builds_app_check_and_config_command() {
        let mut value = base();
        value.app = Some("opencode-quota".into());
        value.check = true;
        value.config = Some(r"D:\Service Manager\data\projects.json".into());
        let command = remote_command(&value);
        assert!(command.contains("run --app opencode-quota --check"));
        assert!(command.contains(r#""D:\Service Manager\data\projects.json""#));
    }

    #[test]
    fn builds_wrapper_command() {
        let mut value = base();
        value.wrapper = Some(r"C:\tools\ServiceManager\scripts\Invoke-UpdaterTask.ps1".into());
        value.app = Some("bsi-qris-api".into());
        value.check = true;
        let command = remote_command(&value);
        assert!(command.starts_with("powershell -NoProfile -ExecutionPolicy Bypass -File"));
        assert!(command.contains("-App bsi-qris-api"));
        assert!(command.contains("-Check"));
    }

    #[test]
    fn quotes_paths_with_spaces() {
        assert_eq!(
            quote_arg(r"C:\Program Files\Service Manager\sm-updater.exe"),
            r#""C:\Program Files\Service Manager\sm-updater.exe""#
        );
        assert_eq!(quote_arg("main"), "main");
    }

    #[test]
    fn builds_ssh_args_with_port_and_identity() {
        let mut value = base();
        value.port = 2222;
        value.identity = Some(r"C:\keys\id_ed25519".into());
        value.ssh_options = vec!["StrictHostKeyChecking=yes".into()];
        let values = ssh_args(&value, "echo ok");
        assert_eq!(
            values,
            [
                "-p",
                "2222",
                "-i",
                r"C:\keys\id_ed25519",
                "-o",
                "StrictHostKeyChecking=yes",
                "EGOV22@10.255.240.160",
                "echo ok"
            ]
        );
    }

    #[test]
    fn parses_defaults_and_action() {
        let parsed = parse_args(&args(&[
            "--host",
            "10.255.240.160",
            "--user",
            "EGOV22",
            "--app",
            "kasirin",
            "--check",
        ]))
        .unwrap();
        assert_eq!(parsed.host, "10.255.240.160");
        assert_eq!(parsed.user, "EGOV22");
        assert_eq!(parsed.app.as_deref(), Some("kasirin"));
        assert!(parsed.check);
        assert_eq!(parsed.port, 22);
    }

    #[test]
    fn requires_host_and_rejects_unknown_argument() {
        assert!(parse_args(&args(&["--user", "EGOV22"])).is_err());
        assert!(parse_args(&args(&["--host", "x", "--bogus"])).is_err());
        assert!(parse_args(&args(&["--host", "x", "--port", "nope"])).is_err());
    }

    #[test]
    fn all_flag_clears_app() {
        let parsed = parse_args(&args(&[
            "--host", "x", "--user", "u", "--app", "a", "--all",
        ]))
        .unwrap();
        assert!(parsed.app.is_none());
    }
}
