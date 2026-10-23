use std::ffi::OsString;

use super::pools::*;
use crate::php::manifest::Os;
use crate::HearthPaths;

#[test]
fn ports_follow_the_version_and_never_touch_herds() {
    assert_eq!(pool_ports(Os::Windows, "8.4"), [19840, 19841, 19842, 19843]);
    assert_eq!(pool_ports(Os::Windows, "8.1"), [19810, 19811, 19812, 19813]);
    assert_eq!(pool_ports(Os::Macos, "8.4"), [19840]);
}

#[test]
fn windows_pools_are_php_cgi_processes_with_hearths_ini() {
    let p = HearthPaths::at(r"C:\h");
    let specs = pool_specs(&p, Os::Windows, "8.4");
    assert_eq!(specs.len(), 4);
    let first = &specs[0];
    assert_eq!(first.key, "php-8.4-0");
    assert!(first.program.ends_with("php-cgi.exe"));
    assert_eq!(
        first.args,
        [
            "-b".into(),
            "127.0.0.1:19840".into(),
            "-c".into(),
            p.config.join("php").join("8.4").join("php.ini").into_os_string()
        ] as [OsString; 4]
    );
    assert!(first.env.iter().any(|(k, v)| k == "PHPRC" && *v == p.config.join("php").join("8.4").into_os_string()));
    assert!(first.env.iter().any(|(k, v)| k == "PHP_FCGI_MAX_REQUESTS" && v == "500"));
    assert_eq!(first.ready_port, Some(19840));
    assert_eq!(specs[3].key, "php-8.4-3");
    assert_eq!(pool_upstreams(&p, Os::Windows, "8.4")[0], "127.0.0.1:19840");
}

#[test]
fn the_macos_pool_is_one_php_fpm_with_a_generated_config() {
    let p = HearthPaths::at("/Users/j/.config/hearth");
    let specs = pool_specs(&p, Os::Macos, "8.4");
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].key, "php-8.4");
    assert!(specs[0].program.ends_with("sbin/php-fpm"));
    assert_eq!(specs[0].args[0], "-F");
    // A socket in the private run folder: only this user can reach PHP (a TCP port would let any
    // local user run PHP files as this user).
    let socket = p.run.join("php-8.4.sock");
    assert_eq!(specs[0].ready_port, None);
    assert_eq!(specs[0].ready_socket, Some(socket.clone()));
    assert_eq!(pool_upstreams(&p, Os::Macos, "8.4"), [format!("unix/{}", socket.display())]);
    let conf = render_fpm_conf(&p, "8.4");
    assert!(conf.contains("\ndaemonize = no\n"), "{conf}");
    assert!(conf.contains(&format!("\nlisten = \"{}\"\nlisten.mode = 0600\n", socket.display())), "{conf}");
    assert!(conf.contains("\npm.max_children = 8\n"), "{conf}");
    assert!(conf.contains("\ncatch_workers_output = yes\n"), "{conf}");
}

#[test]
fn a_site_on_a_removed_version_falls_back_to_the_default() {
    let installed = vec!["8.4".to_string()];
    assert_eq!(effective_php(Some("8.4"), &installed, Some("8.4")), (Some("8.4".into()), None));
    assert_eq!(effective_php(None, &installed, Some("8.4")), (Some("8.4".into()), None));
    assert_eq!(
        effective_php(Some("8.3"), &installed, Some("8.4")),
        (Some("8.4".into()), Some("Uses the default PHP 8.4 (8.3 was removed)".into()))
    );
    assert_eq!(effective_php(Some("8.3"), &[], None), (None, Some("PHP 8.3 is not installed".into())));
    assert_eq!(effective_php(None, &[], None), (None, Some("No PHP version is installed".into())));
}

#[test]
fn a_changed_php_ini_changes_the_pool_so_it_restarts() {
    let d = tempfile::tempdir().unwrap();
    let p = HearthPaths::at(d.path());
    p.ensure().unwrap();
    let ini = p.config.join("php").join("8.4").join("php.ini");
    std::fs::create_dir_all(ini.parent().unwrap()).unwrap();
    std::fs::write(&ini, "memory_limit = 512M\n").unwrap();
    let before = pool_specs(&p, Os::current(), "8.4");
    assert_eq!(pool_specs(&p, Os::current(), "8.4"), before, "same ini: same spec, no restart");
    std::fs::write(&ini, "memory_limit = 1G\n").unwrap();
    assert_ne!(pool_specs(&p, Os::current(), "8.4"), before, "new ini: new spec, the supervisor restarts it");
}
