//! Stands in for a Node build in the launcher tests: prints which install it is, its arguments and
//! the first PATH entry, and exits with the code of an `exit=<n>` argument.

fn main() {
    // `clean-console <program> <args>`: runs the program with Ctrl+C handling switched back on
    // (test runners often start with it ignored, and children inherit that), swallowing the event
    // itself.
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some("clean-console") {
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
        unsafe extern "system" fn swallow(_: u32) -> i32 {
            1
        }
        // SAFETY: clears the inherited ignore setting and registers a do-nothing handler.
        unsafe {
            SetConsoleCtrlHandler(None, 0);
            SetConsoleCtrlHandler(Some(swallow), 1);
        }
        let rest: Vec<String> = std::env::args().skip(2).collect();
        let status = std::process::Command::new(&rest[0]).args(&rest[1..]).status().unwrap();
        std::process::exit(status.code().unwrap_or(1));
    }
    let exe = std::env::current_exe().unwrap();
    // `<install>/node.exe` on Windows, `<install>/bin/node` on macOS.
    let install = if cfg!(windows) { exe.parent().unwrap() } else { exe.parent().unwrap().parent().unwrap() };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let first = std::env::split_paths(&path).next().unwrap_or_default();
    println!("version {}", install.file_name().unwrap().to_string_lossy());
    println!("args {}", args.join("|"));
    println!("path {}", first.display());
    // `ctrlc`: raises Ctrl+C in its console and reports whether it arrived (an ignore setting
    // inherited from the launcher would swallow it).
    #[cfg(windows)]
    if args.iter().any(|a| a == "ctrlc") {
        use std::sync::atomic::{AtomicBool, Ordering};
        use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, SetConsoleCtrlHandler, CTRL_C_EVENT};
        static HIT: AtomicBool = AtomicBool::new(false);
        unsafe extern "system" fn handler(_: u32) -> i32 {
            HIT.store(true, Ordering::SeqCst);
            1
        }
        // SAFETY: registers a handler that only sets a flag; the event goes to this console only.
        unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
            GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0);
        }
        std::thread::sleep(std::time::Duration::from_millis(800));
        println!("ctrl+c {}", HIT.load(Ordering::SeqCst));
    }
    let code = args.iter().find_map(|a| a.strip_prefix("exit=")).map_or(0, |c| c.parse().unwrap());
    std::process::exit(code);
}
