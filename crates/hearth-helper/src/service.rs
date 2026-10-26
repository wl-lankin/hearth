//! The service: reports Running to Windows and serves the pipe until it is told to stop.

use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

use hearth_core::dns::protocol::PIPE;

use crate::pipe;

pub const NAME: &str = "HearthHelper";

define_windows_service!(ffi_service_main, service_main);

/// Hands the process to the service control manager (only works when Windows started it).
pub fn run() -> Result<(), String> {
    service_dispatcher::start(NAME, ffi_service_main).map_err(|e| format!("Not started as a service: {e}"))
}

fn service_main(_args: Vec<OsString>) {
    let stop = Arc::new(AtomicBool::new(false));
    let on_stop = stop.clone();
    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            on_stop.store(true, Ordering::SeqCst);
            // Unblocks the pipe's wait for the next client.
            pipe::wake(PIPE);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let Ok(status) = service_control_handler::register(NAME, handler) else { return };
    let set = |state, accept| {
        let _ = status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(5),
            process_id: None,
        });
    };
    set(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN);
    pipe::serve(PIPE, &pipe::hosts_path(), &stop);
    set(ServiceState::Stopped, ServiceControlAccept::empty());
}
