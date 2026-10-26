//! Installing and removing the service. Both run elevated (the app starts them with "runas").

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows_service::service::{
    Service, ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState, ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

use crate::pipe::hosts_path;
use crate::service::NAME;

/// An admin-only folder, so no user process can replace the program the service runs as SYSTEM.
fn target() -> PathBuf {
    crate::folders::program_files().join("Hearth").join("hearth-helper.exe")
}

fn wait_for(service: &Service, state: ServiceState) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let current = service.query_status().map_err(|e| e.to_string())?.current_state;
        if current == state {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("The Hearth helper did not reach {state:?}"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn stop(service: &Service) -> Result<(), String> {
    if service.query_status().map_err(|e| e.to_string())?.current_state != ServiceState::Stopped {
        let _ = service.stop();
        wait_for(service, ServiceState::Stopped)?;
    }
    Ok(())
}

/// Copies the helper program over the installed one. Windows reports a service stopped a moment
/// before its process lets go of the program, so a refused copy is retried for up to 10 s.
fn copy_program(source: &Path, target: &Path) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match std::fs::copy(source, target) {
            Ok(_) => return Ok(()),
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e),
        }
    }
}

pub fn install() -> Result<(), String> {
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| format!("Could not open the service manager: {e}"))?;
    let target = target();
    let info = ServiceInfo {
        name: NAME.into(),
        display_name: "Hearth Helper".into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: target.clone(),
        launch_arguments: vec![OsString::from(crate::ARG), "service".into()],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let access =
        ServiceAccess::QUERY_STATUS | ServiceAccess::START | ServiceAccess::STOP | ServiceAccess::CHANGE_CONFIG;
    let existing = manager.open_service(NAME, access).ok();
    if let Some(service) = &existing {
        // The running copy holds its program open; stop it before replacing it.
        stop(service)?;
    }
    let source = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if let Err(e) = copy_program(&source, &target) {
        // An update that cannot replace the program keeps the old helper serving.
        if let Some(service) = &existing {
            let _ = service.start::<&str>(&[]);
        }
        return Err(format!("{}: {e}", target.display()));
    }
    let service = match existing {
        Some(service) => {
            service.change_config(&info).map_err(|e| format!("Could not update the Hearth helper: {e}"))?;
            service
        }
        None => {
            manager.create_service(&info, access).map_err(|e| format!("Could not create the Hearth helper: {e}"))?
        }
    };
    let _ = service.set_description("Keeps Hearth's .test names in the hosts file");
    service.start::<&str>(&[]).map_err(|e| format!("Could not start the Hearth helper: {e}"))?;
    wait_for(&service, ServiceState::Running)
}

pub fn uninstall() -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| format!("Could not open the service manager: {e}"))?;
    if let Ok(service) =
        manager.open_service(NAME, ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE)
    {
        stop(&service)?;
        service.delete().map_err(|e| format!("Could not remove the Hearth helper: {e}"))?;
    }
    // Leave the hosts file as it was before Hearth.
    let answer = hearth_core::dns::protocol::handle(br#"{"op":"hosts","names":[]}"#, &hosts_path());
    if let Some(error) = answer.error {
        return Err(error);
    }
    let target = target();
    for _ in 0..20 {
        if std::fs::remove_file(&target).is_ok() || !target.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if let Some(dir) = target.parent() {
        let _ = std::fs::remove_dir(dir);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::windows::fs::OpenOptionsExt;
    use std::time::Duration;

    #[test]
    fn the_copy_waits_for_a_stopping_service_to_let_go_of_its_program() {
        let d = tempfile::tempdir().unwrap();
        let (source, target) = (d.path().join("new.exe"), d.path().join("hearth-helper.exe"));
        std::fs::write(&source, b"new").unwrap();
        std::fs::write(&target, b"old").unwrap();
        // Like the exiting service process: the program is open without sharing for a moment.
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&target).unwrap();
        assert!(std::fs::copy(&source, &target).is_err(), "the held file blocks a plain copy");
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            drop(held);
        });
        super::copy_program(&source, &target).unwrap();
        release.join().unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }
}
