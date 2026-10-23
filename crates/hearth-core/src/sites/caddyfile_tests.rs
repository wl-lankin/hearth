use std::collections::BTreeMap;
use std::path::PathBuf;

use super::caddyfile::*;

fn ctx(routes: Vec<SiteRoute>) -> CaddyContext {
    let mut upstreams = BTreeMap::new();
    upstreams.insert("8.4".to_string(), vec!["127.0.0.1:19840".to_string(), "127.0.0.1:19841".to_string()]);
    upstreams.insert("8.3".to_string(), vec!["unix//Users/j/.config/hearth/run/php-8.3.sock".to_string()]);
    CaddyContext {
        http_port: 8080,
        routes,
        php_upstreams: upstreams,
        log: PathBuf::from(r"C:\h\logs\caddy.log"),
        windows_paths: true,
    }
}

#[test]
fn the_global_block_turns_off_admin_and_https() {
    let file = render(&ctx(vec![]));
    assert!(file.starts_with(
        "{\n\tadmin off\n\tauto_https off\n\tdefault_bind 127.0.0.1 [::1]\n\thttp_port 8080\n\tlog {\n\t\toutput file \"C:/h/logs/caddy.log\"\n\t}\n}\n"
    ), "{file}");
}

#[test]
fn a_php_site_is_served_through_its_pool() {
    let file = render(&ctx(vec![SiteRoute {
        name: "shop".into(),
        target: Route::Php { docroot: PathBuf::from(r"C:\x\shop\public"), minor: "8.4".into() },
    }]));
    assert!(file.contains(
        "\nhttp://shop.test:8080 {\n\troot * \"C:/x/shop/public\"\n\tphp_fastcgi 127.0.0.1:19840 127.0.0.1:19841 {\n\t\tlb_policy least_conn\n\t\tlb_try_duration 2s\n\t}\n\tfile_server\n\tencode gzip\n}\n"
    ), "{file}");
}

#[test]
fn a_site_isolated_to_another_version_uses_that_pool() {
    let file = render(&ctx(vec![SiteRoute {
        name: "old".into(),
        target: Route::Php { docroot: PathBuf::from("/srv/old"), minor: "8.3".into() },
    }]));
    assert!(file.contains("\tphp_fastcgi unix//Users/j/.config/hearth/run/php-8.3.sock {"), "{file}");
}

#[test]
fn a_proxy_forwards_to_its_upstream() {
    let file = render(&ctx(vec![SiteRoute {
        name: "app.shop".into(),
        target: Route::Proxy { upstream: "http://localhost:5173".into() },
    }]));
    assert!(file.contains("\nhttp://app.shop.test:8080 {\n\treverse_proxy http://localhost:5173\n}\n"), "{file}");
}

#[test]
fn awkward_folder_names_stay_inside_one_quoted_string() {
    let file = render(&ctx(vec![SiteRoute {
        name: "odd".into(),
        target: Route::Php { docroot: PathBuf::from(r#"C:\Projects\Jörg {old} "x"\public"#), minor: "8.4".into() },
    }]));
    // Quotes keep it one token; `\{` `\}` stop Caddy from reading `{old}` as a placeholder.
    assert!(file.contains(r#"	root * "C:/Projects/Jörg \{old\} \"x\"/public""#), "{file}");
}

#[test]
fn macos_paths_keep_their_backslashes() {
    let mut c = ctx(vec![SiteRoute {
        name: "odd".into(),
        target: Route::Php { docroot: PathBuf::from(r"/Users/j/a\b/public"), minor: "8.4".into() },
    }]);
    c.windows_paths = false;
    assert!(render(&c).contains(r#"	root * "/Users/j/a\b/public""#), "{}", render(&c));
}

#[test]
fn folders_caddy_cannot_serve_are_named() {
    assert!(servable(std::path::Path::new(r"C:\Projects\shop {v2}\public")).is_ok());
    assert_eq!(
        servable(std::path::Path::new(r"C:\Projects\{$HOME}\public")).unwrap_err(),
        "Caddy cannot serve a folder whose path contains {$"
    );
}

#[test]
fn a_site_that_cannot_be_served_becomes_a_comment() {
    let file = render(&ctx(vec![
        SiteRoute { name: "gone".into(), target: Route::Skip { reason: "Folder missing".into() } },
        SiteRoute {
            name: "new".into(),
            target: Route::Php { docroot: PathBuf::from("/srv/new"), minor: "8.5".into() },
        },
    ]));
    assert!(file.contains("\n# gone: Folder missing\n"), "{file}");
    assert!(file.contains("\n# new: PHP 8.5 is not running\n"), "{file}");
    assert!(!file.contains("http://gone.test") && !file.contains("http://new.test"), "{file}");
}
