//! Stands in for a Node build in the launcher tests: prints which install it is, its arguments and
//! the first PATH entry, and exits with the code of an `exit=<n>` argument.

fn main() {
    let exe = std::env::current_exe().unwrap();
    // `<install>/node.exe` on Windows, `<install>/bin/node` on macOS.
    let install = if cfg!(windows) { exe.parent().unwrap() } else { exe.parent().unwrap().parent().unwrap() };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let first = std::env::split_paths(&path).next().unwrap_or_default();
    println!("version {}", install.file_name().unwrap().to_string_lossy());
    println!("args {}", args.join("|"));
    println!("path {}", first.display());
    let code = args.iter().find_map(|a| a.strip_prefix("exit=")).map_or(0, |c| c.parse().unwrap());
    std::process::exit(code);
}
