//! macOS: the usual ports 80 and 443 for the sites. Caddy runs as the user and may not listen on a
//! port below 1024 on the loopback address, so a packet filter rule forwards 127.0.0.1 and ::1
//! ports 80 and 443 to Caddy's ports. A LaunchDaemon loads the rule into a sub-anchor of
//! `com.apple` (which the system's /etc/pf.conf already loads) at every start; /etc/pf.conf itself
//! is never changed. Setting it up and taking it away each ask for the admin password once.

use std::path::Path;

use crate::CoreError;

pub const LABEL: &str = "de.wolfganglinz.stoke.forward";
pub const PLIST: &str = "/Library/LaunchDaemons/de.wolfganglinz.stoke.forward.plist";
pub const RULES: &str = "/Library/Application Support/Stoke/forward.conf";
/// A sub-anchor of `com.apple/*`, which the stock /etc/pf.conf loads for rdr and filter rules.
pub const ANCHOR: &str = "com.apple/250.StokeForward";

/// The rule text: loopback only, so nothing from the network ever reaches the sites.
pub fn rules_text(http_port: u16, https_port: u16) -> String {
    let mut out = String::from("# Stoke: ports 80 and 443 of this computer go to Stoke's web server.\n");
    for (addr, family) in [("127.0.0.1", "inet"), ("::1", "inet6")] {
        for (from, to) in [(80, http_port), (443, https_port)] {
            out.push_str(&format!(
                "rdr pass on lo0 {family} proto tcp from any to {addr} port {from} -> {addr} port {to}\n"
            ));
        }
    }
    out
}

/// The command that enables pf (adding a reference, so other users of pf are not affected) and
/// loads the rules into Stoke's anchor.
fn load_command() -> String {
    format!("/sbin/pfctl -E -a {ANCHOR} -f {} 2>/dev/null", sh(RULES))
}

/// The LaunchDaemon: loads the rules at every start. The system's own com.apple.pfctl loads
/// /etc/pf.conf at start too, in no fixed order with this one: loading again a little later
/// keeps the rules whichever ran first.
pub fn plist_text() -> String {
    let load = load_command().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>/bin/sh</string>
		<string>-c</string>
		<string>{load}; sleep 15; {load}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
</dict>
</plist>
"#
    )
}

/// Whether the forwarding is set up for these ports. Reads only the installed files: the anchor's
/// rules can only be listed as root.
pub fn active_in(rules: &Path, plist: &Path, http_port: u16, https_port: u16) -> bool {
    plist.is_file() && std::fs::read_to_string(rules).is_ok_and(|text| text == rules_text(http_port, https_port))
}

pub fn active(http_port: u16, https_port: u16) -> bool {
    cfg!(target_os = "macos") && active_in(Path::new(RULES), Path::new(PLIST), http_port, https_port)
}

/// A path as a single-quoted shell word.
fn sh(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

/// The shell command run as root that puts the prepared files in place and loads them.
pub fn setup_command(prepared_rules: &Path, prepared_plist: &Path) -> String {
    let dir = Path::new(RULES).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    [
        format!("mkdir -p {}", sh(&dir)),
        format!("cp {} {}", sh(&prepared_rules.to_string_lossy()), sh(RULES)),
        format!("cp {} {}", sh(&prepared_plist.to_string_lossy()), sh(PLIST)),
        format!("chown root:wheel {} {}", sh(RULES), sh(PLIST)),
        format!("chmod 644 {} {}", sh(RULES), sh(PLIST)),
        // A daemon from an earlier setup (other ports) is replaced.
        format!("(launchctl bootout system/{LABEL} 2>/dev/null; true)"),
        format!("launchctl bootstrap system {}", sh(PLIST)),
        // bootstrap starts the daemon in the background: loading here as well makes the rules
        // certain before the setup returns. Both loads can meet, and then one of them fails:
        // `setup` checks afterwards that port 80 reaches Caddy instead of trusting this status.
        format!("({}; true)", load_command()),
    ]
    .join(" && ")
}

/// The shell command run as root that takes the forwarding away again.
pub fn remove_command() -> String {
    [
        format!("(launchctl bootout system/{LABEL} 2>/dev/null; true)"),
        format!("(/sbin/pfctl -a {ANCHOR} -F all 2>/dev/null; true)"),
        format!("rm -f {} {}", sh(PLIST), sh(RULES)),
    ]
    .join(" && ")
}

/// The AppleScript that runs `command` as root after the admin password.
pub fn admin_script(command: &str) -> String {
    let escaped = command.replace('\\', "\\\\").replace('"', "\\\"");
    format!("do shell script \"{escaped}\" with administrator privileges")
}

/// What answers on a port of this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Stoke's web server.
    Caddy,
    /// Another program, with its `Server` header when it sends one.
    Other(Option<String>),
    /// Nothing listens (Caddy is not running, for example with no sites).
    Nothing,
}

/// Reads the head of an HTTP answer: Caddy names itself in `Server` or `Via`.
pub fn classify(head: &str) -> Answer {
    let mut server = None;
    for line in head.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else { continue };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        if (name == "server" || name == "via") && value.to_ascii_lowercase().contains("caddy") {
            return Answer::Caddy;
        }
        if name == "server" {
            server = Some(value.to_string());
        }
    }
    Answer::Other(server)
}

/// Asks port 80 of 127.0.0.1 for a name no site has.
pub fn probe() -> Answer {
    use std::io::{Read, Write};
    use std::time::Duration;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 80));
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
        return Answer::Nothing;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if stream.write_all(b"HEAD / HTTP/1.0\r\nHost: stoke-forward-check.test\r\n\r\n").is_err() {
        return Answer::Nothing;
    }
    let mut head = Vec::new();
    let _ = stream.take(8192).read_to_end(&mut head);
    if head.is_empty() {
        return Answer::Nothing;
    }
    classify(&String::from_utf8_lossy(&head))
}

#[cfg(target_os = "macos")]
fn run_admin(command: &str, refused: &str) -> Result<(), CoreError> {
    let output = std::process::Command::new("osascript")
        .args(["-e", &admin_script(command)])
        .output()
        .map_err(|e| CoreError::Php(format!("Could not run osascript: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    // -128: the password dialog was cancelled.
    if stderr.contains("(-128)") {
        Err(CoreError::Php(refused.into()))
    } else {
        Err(CoreError::Php(format!("{refused}: {}", stderr.trim())))
    }
}

/// Sets up the forwarding to `http_port` and `https_port`; macOS asks for the admin password.
#[cfg(target_os = "macos")]
pub fn setup(http_port: u16, https_port: u16) -> Result<(), CoreError> {
    let dir = std::env::temp_dir();
    let rules = dir.join("stoke-forward.conf");
    let plist = dir.join("stoke-forward.plist");
    std::fs::write(&rules, rules_text(http_port, https_port)).map_err(|e| CoreError::io(&rules, e))?;
    std::fs::write(&plist, plist_text()).map_err(|e| CoreError::io(&plist, e))?;
    let result = run_admin(&setup_command(&rules, &plist), "Using ports 80 and 443 was not confirmed");
    let _ = std::fs::remove_file(&rules);
    let _ = std::fs::remove_file(&plist);
    result?;
    // The rules take a moment; another program on port 80 means they did not take hold.
    let mut answer = Answer::Nothing;
    for _ in 0..10 {
        answer = probe();
        if answer == Answer::Caddy {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    match answer {
        Answer::Other(server) => Err(CoreError::Php(format!(
            "Port 80 still reaches another program{}, not Stoke",
            server.map(|s| format!(" ({s})")).unwrap_or_default()
        ))),
        // Caddy is not running (no site yet): nothing to check against.
        _ => Ok(()),
    }
}

/// Takes the forwarding away; macOS asks for the admin password.
#[cfg(target_os = "macos")]
pub fn remove() -> Result<(), CoreError> {
    run_admin(&remove_command(), "Going back to Stoke's own ports was not confirmed")
}

#[cfg(not(target_os = "macos"))]
pub fn setup(_http_port: u16, _https_port: u16) -> Result<(), CoreError> {
    Err(CoreError::Php("Forwarding ports 80 and 443 is only needed on a Mac".into()))
}

#[cfg(not(target_os = "macos"))]
pub fn remove() -> Result<(), CoreError> {
    Err(CoreError::Php("Forwarding ports 80 and 443 is only needed on a Mac".into()))
}

#[cfg(test)]
#[path = "forward_tests.rs"]
mod tests;
