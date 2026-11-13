pub mod elevation;
pub mod error;
pub mod scm;

pub use elevation::{command_line, is_elevated, quote_argument, relaunch_elevated, ElevationError};
pub use error::ScmError;
pub use scm::{
    delete_service, query_status, restart_service, service_exists, set_delayed_start, set_startup,
    start_service, stop_service, wait_for_state, wait_for_state_with, ServiceState,
    ServiceStatusInfo, WaitOptions,
};
