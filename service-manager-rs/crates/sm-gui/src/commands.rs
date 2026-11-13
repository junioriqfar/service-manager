use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use sm_core::{validate_service, validate_service_paths, ServiceConfig, ValidationError};
use sm_win::{discovery, ManagerError, ScmError, WaitOptions};
use tauri::{AppHandle, Emitter, State};

use crate::batch::{run_batch, BatchAction, BatchEvent, BatchExecutor, BatchOutcome, BatchState};
use crate::state::{AppState, ServiceRow};
use crate::util;

fn validation_text(errors: Vec<ValidationError>) -> String {
    errors
        .iter()
        .map(|error| error.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn manager_text(error: ManagerError) -> String {
    error.messages().join("\n")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogView {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchOutcomeView {
    pub name: String,
    pub state: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummaryView {
    pub action: String,
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub outcomes: Vec<BatchOutcomeView>,
}

#[tauri::command]
pub fn list_services(state: State<'_, AppState>) -> Vec<ServiceRow> {
    state.rows()
}

#[tauri::command]
pub fn get_warning(state: State<'_, AppState>) -> Option<String> {
    state.take_warning()
}

#[tauri::command]
pub fn get_paths(state: State<'_, AppState>) -> crate::state::PathsView {
    state.paths_view()
}

#[tauri::command]
pub fn is_admin() -> bool {
    sm_win::is_elevated().unwrap_or(false)
}

#[tauri::command]
pub fn elevate(app: AppHandle) -> Result<(), String> {
    if sm_win::is_elevated().unwrap_or(false) {
        return Ok(());
    }
    sm_win::relaunch_elevated(&[]).map_err(|error| error.to_string())?;
    std::thread::sleep(Duration::from_millis(300));
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn read_log(state: State<'_, AppState>, name: String) -> Result<LogView, String> {
    let config = state
        .find(&name)
        .ok_or_else(|| format!("Service '{name}' tidak ada di config."))?;
    let mut paths = Vec::new();
    if !config.stdout_log.trim().is_empty() {
        paths.push(config.stdout_log.trim().to_string());
    }
    if !config.stderr_log.trim().is_empty()
        && !config
            .stderr_log
            .trim()
            .eq_ignore_ascii_case(config.stdout_log.trim())
    {
        paths.push(config.stderr_log.trim().to_string());
    }

    Ok(LogView {
        path: if paths.is_empty() {
            "(belum ada path log)".to_string()
        } else {
            paths.join("; ")
        },
        text: util::build_log_view(&config),
    })
}

#[tauri::command]
pub fn open_log(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let config = state
        .find(&name)
        .ok_or_else(|| format!("Service '{name}' tidak ada di config."))?;
    let path = if !config.stdout_log.trim().is_empty() {
        config.stdout_log.trim()
    } else if !config.stderr_log.trim().is_empty() {
        config.stderr_log.trim()
    } else {
        return Err(format!(
            "Service '{name}' belum punya path log. Atur lewat Edit."
        ));
    };
    util::open_path(std::path::Path::new(path))
}

#[tauri::command]
pub fn open_app_folder(state: State<'_, AppState>) -> Result<(), String> {
    util::open_path(state.paths.home())
}

#[tauri::command]
pub fn open_app_log(state: State<'_, AppState>) -> Result<(), String> {
    state.log("Buka log aplikasi.");
    util::open_path(&state.paths.app_log_path())
}

#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    util::open_path(std::path::Path::new(&path))
}

#[tauri::command]
pub async fn save_service(
    state: State<'_, AppState>,
    mut service: ServiceConfig,
    edit: bool,
) -> Result<Vec<ServiceRow>, String> {
    service.normalize();
    validate_service(&service).map_err(validation_text)?;
    validate_service_paths(&service).map_err(validation_text)?;

    let installed = sm_win::service_exists(&service.name).unwrap_or(false);
    state.upsert(service.clone(), edit)?;
    state.log(&format!(
        "{} service: {}",
        if edit { "Edit" } else { "Tambah" },
        service.name
    ));

    if installed {
        let nssm_path = state.nssm_path.clone();
        let runner = state.runner.clone();
        let service_for_apply = service.clone();
        tokio::task::spawn_blocking(move || {
            sm_win::apply_service(&service_for_apply, &nssm_path, runner.as_ref())
                .map_err(manager_text)
        })
        .await
        .map_err(|error| error.to_string())??;
        state.log(&format!("Terapkan config service: {}", service.name));
    }

    Ok(state.rows())
}

#[tauri::command]
pub async fn delete_service(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<ServiceRow>, String> {
    let config = state
        .find(&name)
        .ok_or_else(|| format!("Service '{name}' tidak ada di config."))?;
    let nssm_path = state.nssm_path.clone();
    let runner = state.runner.clone();
    let config_for_remove = config.clone();

    tokio::task::spawn_blocking(move || {
        sm_win::remove_service(&config_for_remove, &nssm_path, runner.as_ref())
            .map_err(manager_text)
    })
    .await
    .map_err(|error| error.to_string())??;

    state.remove_config(&config.name)?;
    state.log(&format!("Hapus service: {}", config.name));
    Ok(state.rows())
}

#[tauri::command]
pub async fn install_service(
    state: State<'_, AppState>,
    name: String,
    start_after: bool,
) -> Result<Vec<ServiceRow>, String> {
    let config = state
        .find(&name)
        .ok_or_else(|| format!("Service '{name}' tidak ada di config."))?;
    let nssm_path = state.nssm_path.clone();
    let runner = state.runner.clone();
    let config_for_install = config.clone();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        sm_win::install_service(&config_for_install, &nssm_path, runner.as_ref())
            .map_err(manager_text)?;
        if start_after {
            let options = WaitOptions::with_timeout(Duration::from_secs(60));
            sm_win::start_service(&config_for_install.name, &options)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())??;

    state.log(&format!(
        "Install service: {} (start after: {start_after})",
        config.name
    ));
    Ok(state.rows())
}

#[derive(Debug, Clone, Copy)]
enum Operation {
    Start,
    Stop,
    Restart,
}

async fn single_operation(
    state: State<'_, AppState>,
    name: String,
    operation: Operation,
) -> Result<Vec<ServiceRow>, String> {
    if state.find(&name).is_none() {
        return Err(format!("Service '{name}' tidak ada di config."));
    }

    let operation_name = name.clone();
    let action = match operation {
        Operation::Start => "Start",
        Operation::Stop => "Stop",
        Operation::Restart => "Restart",
    };

    tokio::task::spawn_blocking(move || {
        let options = WaitOptions::with_timeout(Duration::from_secs(60));
        match operation {
            Operation::Start => sm_win::start_service(&operation_name, &options),
            Operation::Stop => sm_win::stop_service(&operation_name, &options),
            Operation::Restart => sm_win::restart_service(&operation_name, &options),
        }
        .map(|_| ())
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??;

    state.log(&format!("{action} service: {name}"));
    Ok(state.rows())
}

#[tauri::command]
pub async fn start_service(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<ServiceRow>, String> {
    single_operation(state, name, Operation::Start).await
}

#[tauri::command]
pub async fn stop_service(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<ServiceRow>, String> {
    single_operation(state, name, Operation::Stop).await
}

#[tauri::command]
pub async fn restart_service(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<ServiceRow>, String> {
    single_operation(state, name, Operation::Restart).await
}

struct WinBatchExecutor;

impl BatchExecutor for WinBatchExecutor {
    fn execute(
        &self,
        action: BatchAction,
        service: &ServiceConfig,
        cancel: &Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(), String> {
        let options = WaitOptions {
            timeout: Duration::from_secs(60),
            poll_interval: Duration::from_millis(250),
            cancel: Some(cancel.clone()),
        };
        let result = match action {
            BatchAction::Start => sm_win::start_service(&service.name, &options),
            BatchAction::Stop => sm_win::stop_service(&service.name, &options),
            BatchAction::Restart => sm_win::restart_service(&service.name, &options),
        };
        result
            .map(|_| ())
            .map_err(|error: ScmError| error.to_string())
    }
}

fn parse_batch_action(action: &str) -> Result<BatchAction, String> {
    match action.to_ascii_lowercase().as_str() {
        "start" => Ok(BatchAction::Start),
        "stop" => Ok(BatchAction::Stop),
        "restart" => Ok(BatchAction::Restart),
        other => Err(format!("Aksi batch tidak dikenal: {other}")),
    }
}

fn outcome_view(outcome: BatchOutcome) -> BatchOutcomeView {
    BatchOutcomeView {
        name: outcome.name,
        state: match outcome.state {
            BatchState::Succeeded => "succeeded".to_string(),
            BatchState::Failed => "failed".to_string(),
            BatchState::Cancelled => "cancelled".to_string(),
        },
        message: outcome.message,
    }
}

#[tauri::command]
pub async fn run_batch_action(
    app: AppHandle,
    state: State<'_, AppState>,
    action: String,
) -> Result<BatchSummaryView, String> {
    let batch_action = parse_batch_action(&action)?;
    let services = state.installed_services();
    if services.is_empty() {
        return Err("Tidak ada service terpasang untuk diproses.".to_string());
    }

    let cancel = state
        .begin_batch()
        .ok_or_else(|| "Operasi batch lain sedang berjalan.".to_string())?;
    let total = services.len();
    let _ = app.emit(
        "batch-started",
        serde_json::json!({ "action": action, "total": total }),
    );

    let progress_app = app.clone();
    let progress = Arc::new(move |event: BatchEvent| {
        let _ = progress_app.emit("batch-progress", event);
    });

    let outcomes = run_batch(
        batch_action,
        services,
        Arc::new(WinBatchExecutor),
        cancel,
        4,
        progress,
    )
    .await;

    state.finish_batch();
    state.log(&format!(
        "Batch {action} selesai: {} service.",
        outcomes.len()
    ));

    let mut succeeded = 0;
    let mut failed = 0;
    let mut cancelled = 0;
    let mut views = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        match outcome.state {
            BatchState::Succeeded => succeeded += 1,
            BatchState::Failed => failed += 1,
            BatchState::Cancelled => cancelled += 1,
        }
        views.push(outcome_view(outcome));
    }

    let summary = BatchSummaryView {
        action,
        succeeded,
        failed,
        cancelled,
        outcomes: views,
    };
    let _ = app.emit("batch-finished", &summary);
    Ok(summary)
}

#[tauri::command]
pub fn cancel_batch(state: State<'_, AppState>) {
    state.request_cancel();
}

#[tauri::command]
pub async fn import_candidates(state: State<'_, AppState>) -> Result<Vec<ServiceConfig>, String> {
    let nssm_path = state.nssm_path.clone();
    let runner = state.runner.clone();
    let existing: Vec<String> = state
        .services()
        .iter()
        .map(|service| service.name.to_ascii_lowercase())
        .collect();

    let mut candidates = tokio::task::spawn_blocking(move || {
        discovery::discover_services(&nssm_path, runner.as_ref()).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??;

    candidates.retain(|service| !existing.contains(&service.name.to_ascii_lowercase()));
    Ok(candidates)
}

#[tauri::command]
pub async fn import_services(
    state: State<'_, AppState>,
    names: Vec<String>,
) -> Result<Vec<ServiceRow>, String> {
    if names.is_empty() {
        return Ok(state.rows());
    }
    let nssm_path = state.nssm_path.clone();
    let runner = state.runner.clone();
    let candidates = tokio::task::spawn_blocking(move || {
        discovery::discover_services(&nssm_path, runner.as_ref()).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??;

    let wanted: Vec<String> = names.iter().map(|name| name.to_ascii_lowercase()).collect();
    let mut imported = 0;
    {
        let mut services = state.services();
        for candidate in candidates {
            if !wanted.contains(&candidate.name.to_ascii_lowercase()) {
                continue;
            }
            if services
                .iter()
                .any(|existing| existing.name.eq_ignore_ascii_case(&candidate.name))
            {
                continue;
            }
            services.push(candidate);
            imported += 1;
        }
        state.replace_services(services);
    }
    state.save()?;
    state.log(&format!("Import service: {imported}."));
    Ok(state.rows())
}

#[tauri::command]
pub fn export_config(state: State<'_, AppState>) -> Result<String, String> {
    let path = util::export_services(&state.paths, &state.services())?;
    state.log(&format!("Export config: {}", path.display()));
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn restore_config(
    state: State<'_, AppState>,
    content: String,
) -> Result<Vec<ServiceRow>, String> {
    let mut services =
        sm_config::parse_services_json(&content).map_err(|error| error.to_string())?;
    for service in services.iter_mut() {
        service.normalize();
        validate_service(service).map_err(validation_text)?;
    }

    let mut seen = std::collections::HashSet::new();
    for service in &services {
        if !seen.insert(service.name.to_ascii_lowercase()) {
            return Err(format!("Nama service duplikat di config: {}", service.name));
        }
    }

    state.replace_services(services);
    state.save()?;
    state.log("Restore config dari file.");
    Ok(state.rows())
}
