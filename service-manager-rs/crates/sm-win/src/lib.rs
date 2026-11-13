pub mod discovery;
pub mod elevation;
pub mod error;
pub mod manager;
pub mod native;
pub mod nssm;
pub mod process;
pub mod scm;

pub use discovery::{
    apache_root_from_exe, candidate_from_raw, classify_binary_path, discover_services,
    executable_from_path_name, find_argument, newest_log_file, split_dependencies,
    startup_from_raw, DiscoveryError, NssmValues, RawService,
};
pub use elevation::{command_line, is_elevated, quote_argument, relaunch_elevated, ElevationError};
pub use error::ScmError;
pub use manager::{apply_service, install_service, remove_service, ManagerError};
pub use native::{apply as apply_native, install_plan, NativeCommand, NativeError};
pub use nssm::{apply_commands as nssm_apply_commands, Nssm, NssmCommand, NssmError};
pub use process::{CommandOutput, CommandRunner, ProcessError, SystemRunner};
pub use scm::{
    delete_service, query_status, restart_service, service_exists, set_delayed_start, set_startup,
    start_service, stop_service, wait_for_state, wait_for_state_with, ServiceState,
    ServiceStatusInfo, WaitOptions,
};
