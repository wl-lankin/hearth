use super::*;

#[test]
fn the_rules_forward_80_and_443_on_loopback_only() {
    let rules = rules_text(8080, 8443);
    for line in [
        "rdr pass on lo0 inet proto tcp from any to 127.0.0.1 port 80 -> 127.0.0.1 port 18080\n",
        "rdr pass on lo0 inet proto tcp from any to 127.0.0.1 port 443 -> 127.0.0.1 port 18443\n",
        "rdr pass on lo0 inet6 proto tcp from any to ::1 port 80 -> ::1 port 18080\n",
        "rdr pass on lo0 inet6 proto tcp from any to ::1 port 443 -> ::1 port 18443\n",
    ] {
        assert!(rules.contains(line), "{rules}");
    }
    // Nothing but lo0: the sites stay out of the network's reach.
    assert_eq!(rules.matches("rdr ").count(), 4);
    assert_eq!(rules.matches(" on lo0 ").count(), 4);
}

#[test]
fn forwarded_ports_are_caddys_own_plus_10000_or_minus_near_the_top() {
    assert_eq!((forward_port(8080), forward_port(8443)), (18080, 18443));
    assert_eq!(forward_port(60000), 50000);
}

#[test]
fn the_daemon_loads_the_rules_into_stokes_anchor_twice() {
    let plist = plist_text();
    assert!(plist.contains(&format!("<string>{LABEL}</string>")));
    let load = "/sbin/pfctl -E -a com.apple/250.StokeForward -f '/Library/Application Support/Stoke/forward.conf' 2&gt;/dev/null";
    assert!(plist.contains(&format!("<string>{load}; sleep 15; {load}</string>")), "{plist}");
    assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));
}

#[test]
fn active_only_with_the_daemon_and_rules_for_the_current_ports() {
    let d = tempfile::tempdir().unwrap();
    let rules = d.path().join("forward.conf");
    let plist = d.path().join("forward.plist");
    assert!(!active_in(&rules, &plist, 8080, 8443));
    std::fs::write(&rules, rules_text(8080, 8443)).unwrap();
    assert!(!active_in(&rules, &plist, 8080, 8443), "no daemon yet");
    std::fs::write(&plist, plist_text()).unwrap();
    assert!(active_in(&rules, &plist, 8080, 8443));
    assert!(!active_in(&rules, &plist, 8081, 8443), "set up for other ports");
}

#[test]
fn setup_puts_the_files_in_place_and_loads_them() {
    let cmd = setup_command(Path::new("/tmp/it's/rules"), Path::new("/tmp/p.plist"));
    assert!(cmd.starts_with("mkdir -p '/Library/Application Support/Stoke' && "), "{cmd}");
    assert!(cmd.contains("cp '/tmp/it'\\''s/rules' '/Library/Application Support/Stoke/forward.conf'"), "{cmd}");
    assert!(cmd.contains("cp '/tmp/p.plist' '/Library/LaunchDaemons/de.wolfganglinz.stoke.forward.plist'"));
    assert!(cmd.contains("chown root:wheel "));
    assert!(cmd.contains("(launchctl bootout system/de.wolfganglinz.stoke.forward 2>/dev/null; true) && launchctl bootstrap system '/Library/LaunchDaemons/de.wolfganglinz.stoke.forward.plist'"));
    // The last load may meet the daemon's own: its status is not trusted, setup probes port 80.
    assert!(cmd.ends_with("(/sbin/pfctl -E -a com.apple/250.StokeForward -f '/Library/Application Support/Stoke/forward.conf' 2>/dev/null; true)"), "{cmd}");
}

#[test]
fn remove_unloads_flushes_and_deletes() {
    let cmd = remove_command();
    assert!(cmd.contains("launchctl bootout system/de.wolfganglinz.stoke.forward"));
    assert!(cmd.contains("/sbin/pfctl -a com.apple/250.StokeForward -F all"));
    assert!(cmd.ends_with("rm -f '/Library/LaunchDaemons/de.wolfganglinz.stoke.forward.plist' '/Library/Application Support/Stoke/forward.conf'"));
}

#[test]
fn the_admin_script_keeps_the_command_one_applescript_string() {
    assert_eq!(admin_script(r#"echo "a\b""#), r#"do shell script "echo \"a\\b\"" with administrator privileges"#);
}

#[test]
fn caddy_is_known_by_its_server_or_via_header() {
    assert_eq!(classify("HTTP/1.1 200 OK\r\nServer: Caddy\r\nContent-Length: 0\r\n\r\n"), Answer::Caddy);
    assert_eq!(classify("HTTP/1.1 502 Bad Gateway\r\nVia: 0.0 Caddy\r\n\r\n"), Answer::Caddy);
    assert_eq!(
        classify("HTTP/1.1 404 Not Found\r\nServer: nginx/1.29.1\r\n\r\n"),
        Answer::Other(Some("nginx/1.29.1".into()))
    );
    assert_eq!(classify("HTTP/1.0 200 OK\r\n\r\n"), Answer::Other(None));
}
