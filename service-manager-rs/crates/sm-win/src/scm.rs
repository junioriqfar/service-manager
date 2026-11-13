use std::ffi::c_void;
use std::fmt;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use sm_core::StartupMode;
use windows::core::{Error as WinError, BOOL, PCWSTR};
use windows::Win32::Foundation::{
    ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_CANNOT_ACCEPT_CTRL, ERROR_SERVICE_DOES_NOT_EXIST,
    ERROR_SERVICE_NOT_ACTIVE,
};
use windows::Win32::System::Services::{
    ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, ControlService, DeleteService,
    OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, StartServiceW, ENUM_SERVICE_TYPE,
    SC_MANAGER_CONNECT, SC_MANAGER_ENUMERATE_SERVICE, SC_STATUS_PROCESS_INFO, SERVICE_AUTO_START,
    SERVICE_CHANGE_CONFIG, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_CONTINUE_PENDING,
    SERVICE_CONTROL_STOP, SERVICE_DELAYED_AUTO_START_INFO, SERVICE_DEMAND_START, SERVICE_ERROR,
    SERVICE_NO_CHANGE, SERVICE_PAUSED, SERVICE_PAUSE_PENDING, SERVICE_QUERY_STATUS,
    SERVICE_RUNNING, SERVICE_START, SERVICE_START_PENDING, SERVICE_START_TYPE, SERVICE_STATUS,
    SERVICE_STATUS_PROCESS, SERVICE_STOP, SERVICE_STOPPED, SERVICE_STOP_PENDING,
};

use crate::error::ScmError;

// DELETE access right (0x00010000) tidak diekspor sebagai konstanta service.
const SERVICE_DELETE_ACCESS: u32 = 0x0001_0000;

type Result<T> = std::result::Result<T, ScmError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Stopped,
    StartPending,
    StopPending,
    Running,
    ContinuePending,
    PausePending,
    Paused,
    Unknown(u32),
}

impl ServiceState {
    pub fn from_raw(value: u32) -> Self {
        match value {
            value if value == SERVICE_STOPPED.0 => Self::Stopped,
            value if value == SERVICE_START_PENDING.0 => Self::StartPending,
            value if value == SERVICE_STOP_PENDING.0 => Self::StopPending,
            value if value == SERVICE_RUNNING.0 => Self::Running,
            value if value == SERVICE_CONTINUE_PENDING.0 => Self::ContinuePending,
            value if value == SERVICE_PAUSE_PENDING.0 => Self::PausePending,
            value if value == SERVICE_PAUSED.0 => Self::Paused,
            other => Self::Unknown(other),
        }
    }

    pub fn is_running(self) -> bool {
        self == Self::Running
    }

    pub fn is_pending(self) -> bool {
        matches!(
            self,
            Self::StartPending | Self::StopPending | Self::ContinuePending | Self::PausePending
        )
    }
}

impl fmt::Display for ServiceState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stopped => write!(formatter, "Stopped"),
            Self::StartPending => write!(formatter, "Start Pending"),
            Self::StopPending => write!(formatter, "Stop Pending"),
            Self::Running => write!(formatter, "Running"),
            Self::ContinuePending => write!(formatter, "Continue Pending"),
            Self::PausePending => write!(formatter, "Pause Pending"),
            Self::Paused => write!(formatter, "Paused"),
            Self::Unknown(code) => write!(formatter, "Unknown({code})"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceStatusInfo {
    pub state: ServiceState,
    pub process_id: u32,
}

#[derive(Debug, Clone)]
pub struct WaitOptions {
    pub timeout: Duration,
    pub poll_interval: Duration,
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(60),
            poll_interval: Duration::from_millis(250),
            cancel: None,
        }
    }
}

impl WaitOptions {
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            ..Self::default()
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .map(|flag| flag.load(Ordering::SeqCst))
            .unwrap_or(false)
    }
}

pub(crate) struct ScmHandle(pub(crate) windows::Win32::System::Services::SC_HANDLE);

impl Drop for ScmHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseServiceHandle(self.0) };
    }
}

pub(crate) struct ServiceHandle(pub(crate) windows::Win32::System::Services::SC_HANDLE);

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseServiceHandle(self.0) };
    }
}

pub(crate) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn win32_code(error: &WinError) -> u32 {
    error.code().0 as u32 & 0xFFFF
}

fn map_service_error(name: &str, error: WinError) -> ScmError {
    if win32_code(&error) == ERROR_SERVICE_DOES_NOT_EXIST.0 {
        ScmError::NotFound(name.to_string())
    } else {
        ScmError::Windows(format!("Service '{name}': {error}"))
    }
}

pub(crate) fn open_scm() -> Result<ScmHandle> {
    let handle = unsafe {
        OpenSCManagerW(
            PCWSTR::null(),
            PCWSTR::null(),
            SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE,
        )
    }
    .map_err(|error| {
        ScmError::Windows(format!("Gagal membuka Service Control Manager: {error}"))
    })?;
    Ok(ScmHandle(handle))
}

pub(crate) fn open_service(scm: &ScmHandle, name: &str, access: u32) -> Result<ServiceHandle> {
    let name_wide = wide(name);
    let handle = unsafe { OpenServiceW(scm.0, PCWSTR(name_wide.as_ptr()), access) }
        .map_err(|error| map_service_error(name, error))?;
    Ok(ServiceHandle(handle))
}

pub fn query_status(name: &str) -> Result<ServiceStatusInfo> {
    let scm = open_scm()?;
    let service = open_service(&scm, name, SERVICE_QUERY_STATUS)?;
    query_status_handle(name, &service)
}

fn query_status_handle(name: &str, service: &ServiceHandle) -> Result<ServiceStatusInfo> {
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut needed = 0u32;
    let buffer = unsafe {
        std::slice::from_raw_parts_mut(
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast::<u8>(),
            size_of::<SERVICE_STATUS_PROCESS>(),
        )
    };

    unsafe { QueryServiceStatusEx(service.0, SC_STATUS_PROCESS_INFO, Some(buffer), &mut needed) }
        .map_err(|error| map_service_error(name, error))?;

    Ok(ServiceStatusInfo {
        state: ServiceState::from_raw(status.dwCurrentState.0),
        process_id: status.dwProcessId,
    })
}

pub fn service_exists(name: &str) -> Result<bool> {
    match query_status(name) {
        Ok(_) => Ok(true),
        Err(ScmError::NotFound(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn wait_for_state_with<F>(
    name: &str,
    desired: ServiceState,
    options: &WaitOptions,
    mut query: F,
) -> Result<ServiceStatusInfo>
where
    F: FnMut() -> Result<ServiceStatusInfo>,
{
    let deadline = Instant::now() + options.timeout;

    loop {
        if options.is_cancelled() {
            return Err(ScmError::Cancelled);
        }

        let info = query()?;
        if info.state == desired {
            return Ok(info);
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(ScmError::Timeout(
                name.to_string(),
                options.timeout,
                info.state,
            ));
        }

        let remaining = deadline.saturating_duration_since(now);
        thread::sleep(options.poll_interval.min(remaining));
    }
}

pub fn wait_for_state(
    name: &str,
    desired: ServiceState,
    options: &WaitOptions,
) -> Result<ServiceStatusInfo> {
    wait_for_state_with(name, desired, options, || query_status(name))
}

pub fn start_service(name: &str, options: &WaitOptions) -> Result<ServiceStatusInfo> {
    let scm = open_scm()?;
    let service = open_service(&scm, name, SERVICE_START | SERVICE_QUERY_STATUS)?;

    match unsafe { StartServiceW(service.0, None) } {
        Ok(()) => {}
        Err(error) if win32_code(&error) == ERROR_SERVICE_ALREADY_RUNNING.0 => {
            return Ok(ServiceStatusInfo {
                state: ServiceState::Running,
                process_id: query_status(name).map(|info| info.process_id).unwrap_or(0),
            });
        }
        Err(error) => return Err(map_service_error(name, error)),
    }

    wait_for_state(name, ServiceState::Running, options)
}

pub fn stop_service(name: &str, options: &WaitOptions) -> Result<ServiceStatusInfo> {
    let scm = open_scm()?;
    let service = open_service(&scm, name, SERVICE_STOP | SERVICE_QUERY_STATUS)?;
    let mut status = SERVICE_STATUS::default();

    match unsafe { ControlService(service.0, SERVICE_CONTROL_STOP, &mut status) } {
        Ok(()) => {}
        Err(error) if win32_code(&error) == ERROR_SERVICE_NOT_ACTIVE.0 => {
            return Ok(ServiceStatusInfo {
                state: ServiceState::Stopped,
                process_id: 0,
            });
        }
        Err(error) if win32_code(&error) == ERROR_SERVICE_CANNOT_ACCEPT_CTRL.0 => {
            return Err(ScmError::Windows(format!(
                "Service '{name}' tidak menerima kontrol stop: {error}"
            )));
        }
        Err(error) => return Err(map_service_error(name, error)),
    }

    wait_for_state(name, ServiceState::Stopped, options)
}

pub fn restart_service(name: &str, options: &WaitOptions) -> Result<ServiceStatusInfo> {
    stop_service(name, options)?;
    start_service(name, options)
}

pub fn delete_service(name: &str) -> Result<()> {
    let scm = open_scm()?;
    match open_service(&scm, name, SERVICE_DELETE_ACCESS) {
        Ok(service) => {
            unsafe { DeleteService(service.0) }.map_err(|error| map_service_error(name, error))?;
            Ok(())
        }
        Err(ScmError::NotFound(_)) => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn set_startup(name: &str, mode: StartupMode) -> Result<()> {
    let scm = open_scm()?;
    let service = open_service(&scm, name, SERVICE_CHANGE_CONFIG)?;
    let mode = mode.normalized();

    let start_type: SERVICE_START_TYPE = match mode {
        StartupMode::Auto | StartupMode::Delayed => SERVICE_AUTO_START,
        StartupMode::Manual => SERVICE_DEMAND_START,
        StartupMode::Unknown => SERVICE_AUTO_START,
    };

    unsafe {
        ChangeServiceConfigW(
            service.0,
            ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
            start_type,
            SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        )
    }
    .map_err(|error| map_service_error(name, error))?;

    set_delayed_start_handle(name, &service, mode == StartupMode::Delayed)
}

fn set_delayed_start_handle(name: &str, service: &ServiceHandle, enabled: bool) -> Result<()> {
    let info = SERVICE_DELAYED_AUTO_START_INFO {
        fDelayedAutostart: BOOL(if enabled { 1 } else { 0 }),
    };
    unsafe {
        ChangeServiceConfig2W(
            service.0,
            SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
            Some((&info as *const SERVICE_DELAYED_AUTO_START_INFO).cast::<c_void>()),
        )
    }
    .map_err(|error| map_service_error(name, error))
}

pub fn set_delayed_start(name: &str, enabled: bool) -> Result<()> {
    let scm = open_scm()?;
    let service = open_service(&scm, name, SERVICE_CHANGE_CONFIG)?;
    set_delayed_start_handle(name, &service, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn maps_raw_states() {
        assert_eq!(
            ServiceState::from_raw(SERVICE_RUNNING.0),
            ServiceState::Running
        );
        assert_eq!(
            ServiceState::from_raw(SERVICE_STOPPED.0),
            ServiceState::Stopped
        );
        assert_eq!(
            ServiceState::from_raw(SERVICE_START_PENDING.0),
            ServiceState::StartPending
        );
        assert_eq!(ServiceState::from_raw(999), ServiceState::Unknown(999));
        assert!(ServiceState::StartPending.is_pending());
        assert!(!ServiceState::Running.is_pending());
    }

    #[test]
    fn wait_succeeds_after_pending_states() {
        let attempts = AtomicUsize::new(0);
        let options = WaitOptions {
            timeout: Duration::from_millis(500),
            poll_interval: Duration::from_millis(1),
            cancel: None,
        };
        let info = wait_for_state_with("demo", ServiceState::Running, &options, || {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
            let state = match attempt {
                0 => ServiceState::StartPending,
                1 => ServiceState::StartPending,
                _ => ServiceState::Running,
            };
            Ok(ServiceStatusInfo {
                state,
                process_id: 42,
            })
        })
        .unwrap();
        assert_eq!(info.state, ServiceState::Running);
        assert_eq!(info.process_id, 42);
    }

    #[test]
    fn wait_times_out_with_last_state() {
        let options = WaitOptions {
            timeout: Duration::from_millis(5),
            poll_interval: Duration::from_millis(1),
            cancel: None,
        };
        let error = wait_for_state_with("demo", ServiceState::Running, &options, || {
            Ok(ServiceStatusInfo {
                state: ServiceState::StartPending,
                process_id: 0,
            })
        })
        .unwrap_err();
        match error {
            ScmError::Timeout(name, _, state) => {
                assert_eq!(name, "demo");
                assert_eq!(state, ServiceState::StartPending);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn wait_can_be_cancelled() {
        let cancel = Arc::new(AtomicBool::new(true));
        let options = WaitOptions {
            timeout: Duration::from_secs(1),
            poll_interval: Duration::from_millis(1),
            cancel: Some(cancel),
        };
        let error = wait_for_state_with("demo", ServiceState::Running, &options, || {
            Ok(ServiceStatusInfo {
                state: ServiceState::StartPending,
                process_id: 0,
            })
        })
        .unwrap_err();
        assert_eq!(error, ScmError::Cancelled);
    }

    #[test]
    fn read_only_query_for_existing_service() {
        // EventLog selalu ada pada Windows dan tidak membutuhkan Administrator.
        let status = query_status("EventLog").expect("EventLog harus bisa dibaca");
        assert!(status.state.is_running() || status.state.is_pending());
        assert!(service_exists("EventLog").unwrap());
    }

    #[test]
    fn missing_service_is_reported_as_not_found() {
        let name = format!("sm-does-not-exist-{}", std::process::id());
        let error = query_status(&name).unwrap_err();
        assert!(matches!(error, ScmError::NotFound(_)));
        assert!(!service_exists(&name).unwrap());
    }
}
