use sm_config::ConfigPaths;
use sm_core::ServiceConfig;
use sm_win::ScmError;

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub home: String,
    pub services_path: String,
    pub projects_path: String,
    pub nssm_path: String,
    pub nssm_exists: bool,
    pub admin: bool,
    pub warning: Option<String>,
    pub entries: Vec<DiagnosticEntry>,
}

#[derive(Debug, Clone)]
pub struct DiagnosticEntry {
    pub name: String,
    pub display_name: String,
    pub service_type: String,
    pub executable: String,
    pub state: String,
}

pub fn collect(paths: &ConfigPaths) -> Diagnostic {
    let loaded = sm_config::load_services_lenient(&paths.services_path(), &paths.backup_dir());
    let nssm_path = paths.nssm_path();

    let entries = loaded.services.iter().map(entry).collect();

    Diagnostic {
        home: paths.home().to_string_lossy().to_string(),
        services_path: paths.services_path().to_string_lossy().to_string(),
        projects_path: paths.projects_path().to_string_lossy().to_string(),
        nssm_exists: nssm_path.is_file(),
        nssm_path: nssm_path.to_string_lossy().to_string(),
        admin: sm_win::is_elevated().unwrap_or(false),
        warning: loaded.warning,
        entries,
    }
}

fn entry(service: &ServiceConfig) -> DiagnosticEntry {
    let state = match sm_win::query_status(&service.name) {
        Ok(info) => info.state.to_string(),
        Err(ScmError::NotFound(_)) => "Not Installed".to_string(),
        Err(error) => format!("Unknown ({error})"),
    };
    DiagnosticEntry {
        name: service.name.clone(),
        display_name: service.display_name.clone(),
        service_type: service.service_type.label().to_string(),
        executable: service.executable.clone(),
        state,
    }
}

pub fn render(diagnostic: &Diagnostic) -> String {
    let mut lines = Vec::new();
    lines.push("Service Manager - diagnosa".to_string());
    lines.push(format!("Home          : {}", diagnostic.home));
    lines.push(format!("Config        : {}", diagnostic.services_path));
    lines.push(format!("Projects      : {}", diagnostic.projects_path));
    lines.push(format!(
        "NSSM          : {} (ada: {})",
        diagnostic.nssm_path, diagnostic.nssm_exists
    ));
    lines.push(format!("Administrator : {}", diagnostic.admin));
    if let Some(warning) = &diagnostic.warning {
        lines.push(format!("Peringatan    : {warning}"));
    }
    lines.push(format!("Service       : {}", diagnostic.entries.len()));
    if diagnostic.entries.is_empty() {
        lines.push("  (tidak ada service di config)".to_string());
    } else {
        for entry in &diagnostic.entries {
            lines.push(format!(
                "  - [{}] {} / {} ({}) -> {}",
                entry.state, entry.name, entry.display_name, entry.service_type, entry.executable
            ));
        }
    }
    lines.join("\n")
}

pub fn run() -> i32 {
    let paths = ConfigPaths::detect();
    let diagnostic = collect(&paths);
    println!("{}", render(&diagnostic));
    if diagnostic.warning.is_some() {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_empty_config_diagnostic() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        let diagnostic = collect(&paths);
        let text = render(&diagnostic);
        assert!(text.contains("Service Manager - diagnosa"));
        assert!(text.contains("tidak ada service"));
        assert_eq!(diagnostic.entries.len(), 0);
    }

    #[test]
    fn renders_service_entries() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::write(
            paths.services_path(),
            r#"{"version":1,"services":[{"name":"demo","executable":"C:\\demo.exe","serviceType":"nssm"}]}"#,
        )
        .unwrap();
        let diagnostic = collect(&paths);
        let text = render(&diagnostic);
        assert!(text.contains("demo"));
        assert!(text.contains("NSSM"));
        assert_eq!(diagnostic.entries.len(), 1);
    }
}
