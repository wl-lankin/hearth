//! Hearth's Windows helper: a small service, installed once with one UAC prompt, that keeps
//! Hearth's block in the hosts file current. It runs from the Hearth app binary started as
//! `hearth-app --hearth-helper <install|uninstall|service>`; what it accepts is defined in
//! `hearth_core::dns::protocol`.

/// The first argument that switches the app binary into helper mode.
pub const ARG: &str = "--hearth-helper";

#[cfg(windows)]
mod install;
#[cfg(windows)]
mod pipe;
#[cfg(windows)]
mod service;

/// Runs the helper when the process was started in helper mode, returning its exit code;
/// `None` for a normal start.
pub fn run_from_args() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(ARG) {
        return None;
    }
    #[cfg(windows)]
    {
        let verb = args.next().unwrap_or_default();
        Some(match verb.as_str() {
            "install" => report(install::install()),
            "uninstall" => report(install::uninstall()),
            "service" => report(service::run()),
            _ => 2,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = args;
        Some(2)
    }
}

#[cfg(windows)]
fn report(result: Result<(), String>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
