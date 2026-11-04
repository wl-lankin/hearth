//! Finding and stopping Herd's processes, and who holds a port. Processes are picked by the folder
//! their program lives in (Herd's install and home folders), and stopped by their process id.

use std::path::Path;

fn output(program: &str, args: &[&str]) -> String {
    crate::workspaces::command(program)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Every running process as (id, program path).
pub fn processes() -> Vec<(u32, String)> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let script = "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ProcessId)|$($_.ExecutablePath)\" }";
    output("powershell", &["-NoProfile", "-NonInteractive", "-Command", script])
        .lines()
        .filter_map(|l| {
            let (pid, path) = l.trim().split_once('|')?;
            Some((pid.parse().ok()?, path.to_string()))
        })
        .filter(|(_, path)| !path.is_empty())
        .collect()
}

/// Whether `program` lies inside one of `folders` (compared without case, as Windows does).
pub fn inside(program: &str, folders: &[&Path]) -> bool {
    let program = program.to_ascii_lowercase().replace('/', "\\");
    folders.iter().any(|f| {
        let folder = f.to_string_lossy().to_ascii_lowercase().replace('/', "\\");
        let folder = folder.trim_end_matches('\\');
        program.starts_with(&format!("{folder}\\"))
    })
}

/// Stops the process `pid` and the processes it started.
pub fn stop_pid(pid: u32) {
    if cfg!(windows) {
        let _ = output("taskkill", &["/PID", &pid.to_string(), "/T", "/F"]);
    }
}

/// The id of the process listening on `port` (TCP), from `netstat`.
pub fn listener(port: u16) -> Option<u32> {
    if !cfg!(windows) {
        return None;
    }
    output("netstat", &["-ano", "-p", "TCP"]).lines().find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let (local, state, pid) = (cols.get(1)?, cols.get(3)?, cols.get(4)?);
        (local.rsplit(':').next()? == port.to_string() && state.eq_ignore_ascii_case("LISTENING"))
            .then(|| pid.parse().ok())
            .flatten()
    })
}

/// A name for the program holding `port`: its file name, or its process id.
pub fn holder(port: u16) -> Option<String> {
    let pid = listener(port)?;
    let name = processes()
        .into_iter()
        .find(|(p, _)| *p == pid)
        .and_then(|(_, path)| Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()));
    Some(name.unwrap_or_else(|| format!("process {pid}")))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::inside;

    #[test]
    fn a_program_is_inside_a_folder_only_below_it() {
        let herd = Path::new(r"C:\Program Files\Herd");
        assert!(inside(r"c:\program files\herd\Herd.exe", &[herd]));
        assert!(inside(r"C:\Program Files\Herd\resources\bin\nginx.exe", &[herd]));
        assert!(!inside(r"C:\Program Files\HerdOther\x.exe", &[herd]), "a folder with a longer name is no match");
        assert!(!inside(r"C:\Program Files\Hearth\hearth.exe", &[herd]));
    }
}
