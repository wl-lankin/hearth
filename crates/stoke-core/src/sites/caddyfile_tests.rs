use std::collections::BTreeMap;
use std::path::PathBuf;

use super::caddyfile::*;

fn ctx(routes: Vec<SiteRoute>) -> CaddyContext {
    let mut upstreams = BTreeMap::new();
    upstreams.insert("8.4".to_string(), vec!["127.0.0.1:19840".to_string(), "127.0.0.1:19841".to_string()]);
    upstreams.insert("8.3".to_string(), vec!["unix//Users/j/.config/stoke/run/php-8.3.sock".to_string()]);
    CaddyContext {
        http_port: 8080,
        https_port: 8443,
        routes,
        php_upstreams: upstreams,
        log: PathBuf::from(r"C:\h\logs\caddy.log"),
        windows_paths: true,
        forwarded: false,
    }
}

#[test]
fn the_global_block_turns_off_admin_and_https() {
    let file = render(&ctx(vec![]));
    assert!(file.starts_with(
        "{\n\tadmin off\n\tauto_https off\n\tdefault_bind 127.0.0.1 [::1]\n\thttp_port 8080\n\thttps_port 8443\n\tlog {\n\t\toutput file \"C:/h/logs/caddy.log\"\n\t}\n}\n"
    ), "{file}");
}

#[test]
fn a_php_site_is_served_through_its_pool() {
    let file = render(&ctx(vec![SiteRoute {
        name: "shop".into(),
        tls: None,
        target: Route::Php {
            docroot: PathBuf::from(r"C:\x\shop\public"),
            minor: "8.4".into(),
            xdebug: false,
            spx: None,
            spx_ui: None,
        },
    }]));
    assert!(file.contains(
        "\nhttp://shop.test:8080 {\n\troot * \"C:/x/shop/public\"\n\tphp_fastcgi 127.0.0.1:19840 127.0.0.1:19841 {\n\t\ttry_files {path} {path}/index.php {path}/index.html index.php\n\t\tlb_policy least_conn\n\t\tlb_try_duration 2s\n\t}\n\tfile_server\n\tencode gzip\n}\n"
    ), "{file}");
}

#[test]
fn a_site_isolated_to_another_version_uses_that_pool() {
    let file = render(&ctx(vec![SiteRoute {
        name: "old".into(),
        tls: None,
        target: Route::Php {
            docroot: PathBuf::from("/srv/old"),
            minor: "8.3".into(),
            xdebug: false,
            spx: None,
            spx_ui: None,
        },
    }]));
    assert!(file.contains("\tphp_fastcgi unix//Users/j/.config/stoke/run/php-8.3.sock {"), "{file}");
}

#[test]
fn a_proxy_forwards_to_its_upstream() {
    let file = render(&ctx(vec![SiteRoute {
        name: "app.shop".into(),
        tls: None,
        target: Route::Proxy { upstream: "http://localhost:5173".into() },
    }]));
    // Dev servers like Vite 6 refuse unknown Host headers: send the upstream's own host.
    assert!(
        file.contains(
            "\nhttp://app.shop.test:8080 {\n\treverse_proxy http://localhost:5173 {\n\t\theader_up Host {upstream_hostport}\n\t}\n}\n"
        ),
        "{file}"
    );
}

#[test]
fn awkward_folder_names_stay_inside_one_quoted_string() {
    let file = render(&ctx(vec![SiteRoute {
        name: "odd".into(),
        tls: None,
        target: Route::Php {
            docroot: PathBuf::from(r#"C:\Projects\Jörg {old} "x"\public"#),
            minor: "8.4".into(),
            xdebug: false,
            spx: None,
            spx_ui: None,
        },
    }]));
    // Quotes keep it one token; `\{` `\}` stop Caddy from reading `{old}` as a placeholder.
    assert!(file.contains(r#"	root * "C:/Projects/Jörg \{old\} \"x\"/public""#), "{file}");
}

#[test]
fn folders_caddy_cannot_serve_are_named() {
    use std::path::Path;
    assert!(servable(Path::new(r"C:\Projects\shop {v2}\public"), true).is_ok());
    assert_eq!(
        servable(Path::new(r"C:\Projects\{$HOME}\public"), true).unwrap_err(),
        "Caddy cannot serve a folder whose path contains {$"
    );
    // On macOS a backslash is part of a name; one right before the closing quote would end the
    // quoted path early and let the rest become Caddyfile directives.
    assert_eq!(
        servable(Path::new(r"/Users/j/evil\"), false).unwrap_err(),
        "Caddy cannot serve a folder whose path contains a backslash"
    );
    assert_eq!(
        servable(Path::new("/Users/j/a\nb"), false).unwrap_err(),
        "Caddy cannot serve a folder whose path contains a line break or control character"
    );
    assert!(servable(Path::new("C:\\Projects\\a\rb"), true).is_err());
}

#[test]
fn a_site_that_cannot_be_served_becomes_a_comment() {
    let file = render(&ctx(vec![
        SiteRoute { name: "gone".into(), target: Route::Skip { reason: "Folder missing".into() }, tls: None },
        SiteRoute {
            name: "new".into(),
            tls: None,
            target: Route::Php {
                docroot: PathBuf::from("/srv/new"),
                minor: "8.5".into(),
                xdebug: false,
                spx: None,
                spx_ui: None,
            },
        },
    ]));
    assert!(file.contains("\n# gone: Folder missing\n"), "{file}");
    assert!(file.contains("\n# new: PHP 8.5 is not running\n"), "{file}");
    assert!(!file.contains("http://gone.test") && !file.contains("http://new.test"), "{file}");
}

fn secured(name: &str, target: Route) -> SiteRoute {
    SiteRoute {
        name: name.into(),
        target,
        tls: Some((
            PathBuf::from(format!(r"C:\h\tls\{{x}}\{name}.crt")),
            PathBuf::from(format!(r"C:\h\tls\{{x}}\{name}.key")),
        )),
    }
}

#[test]
fn a_secured_php_site_is_served_over_https_and_http_redirects() {
    let file = render(&ctx(vec![secured(
        "shop",
        Route::Php {
            docroot: PathBuf::from(r"C:\x\shop\public"),
            minor: "8.4".into(),
            xdebug: false,
            spx: None,
            spx_ui: None,
        },
    )]));
    assert!(file.contains(
        "\nhttps://shop.test:8443 {\n\ttls \"C:/h/tls/\\{x\\}/shop.crt\" \"C:/h/tls/\\{x\\}/shop.key\"\n\troot * \"C:/x/shop/public\"\n\tphp_fastcgi 127.0.0.1:19840 127.0.0.1:19841 {\n\t\ttry_files {path} {path}/index.php {path}/index.html index.php\n\t\tlb_policy least_conn\n\t\tlb_try_duration 2s\n\t}\n\tfile_server\n\tencode gzip\n}\n"
    ), "{file}");
    assert!(file.contains("\nhttp://shop.test:8080 {\n\tredir https://shop.test:8443{uri}\n}\n"), "{file}");
}

#[test]
fn with_80_and_443_forwarded_the_redirect_drops_the_port() {
    let mut context = ctx(vec![secured("vite", Route::Proxy { upstream: "http://localhost:5173".into() })]);
    context.forwarded = true;
    let file = render(&context);
    assert!(file.contains("\nhttp://vite.test:8080 {\n\tredir https://vite.test{uri}\n}\n"), "{file}");
    // Caddy keeps listening on its own ports: the forwarding brings 80 and 443 there.
    assert!(file.contains("\nhttps://vite.test:8443 {\n"), "{file}");
}

#[test]
fn a_secured_proxy_keeps_its_upstream() {
    let file = render(&ctx(vec![secured("vite", Route::Proxy { upstream: "http://localhost:5173".into() })]));
    assert!(file.contains(
        "\nhttps://vite.test:8443 {\n\ttls \"C:/h/tls/\\{x\\}/vite.crt\" \"C:/h/tls/\\{x\\}/vite.key\"\n\treverse_proxy http://localhost:5173 {\n\t\theader_up Host {upstream_hostport}\n\t}\n}\n"
    ), "{file}");
}

#[test]
fn a_debugged_site_passes_the_xdebug_trigger() {
    let file = render(&ctx(vec![
        SiteRoute {
            name: "shop".into(),
            tls: None,
            target: Route::Php {
                docroot: PathBuf::from("/srv/shop/public"),
                minor: "8.4".into(),
                xdebug: true,
                spx: None,
                spx_ui: None,
            },
        },
        SiteRoute {
            name: "blog".into(),
            tls: None,
            target: Route::Php {
                docroot: PathBuf::from("/srv/blog/public"),
                minor: "8.4".into(),
                xdebug: false,
                spx: None,
                spx_ui: None,
            },
        },
    ]));
    assert!(
        file.contains(
            "\tphp_fastcgi 127.0.0.1:19840 127.0.0.1:19841 {\n\t\tenv XDEBUG_TRIGGER stoke\n\t\ttry_files {path} {path}/index.php {path}/index.html index.php\n\t\tlb_policy least_conn\n"
        ),
        "{file}"
    );
    assert_eq!(file.matches("XDEBUG_TRIGGER").count(), 1, "only the debugged site: {file}");
}

#[test]
fn a_profiled_site_passes_the_spx_switches() {
    let route = |name: &str, spx: Option<String>| SiteRoute {
        name: name.into(),
        tls: None,
        target: Route::Php {
            docroot: PathBuf::from(format!("/srv/{name}")),
            minor: "8.4".into(),
            xdebug: false,
            spx,
            spx_ui: None,
        },
    };
    let file = render(&ctx(vec![route("shop", Some("k123".into())), route("blog", None)]));
    assert!(
        file.contains("\tphp_fastcgi 127.0.0.1:19840 127.0.0.1:19841 {\n\t\tenv HTTP_SPX_ENABLED 1\n\t\tenv HTTP_SPX_KEY k123\n\t\tenv HTTP_SPX_AUTO_START 1\n\t\ttry_files {path} {path}/index.php {path}/index.html index.php\n\t\tlb_policy least_conn\n"),
        "{file}"
    );
    assert_eq!(file.matches("HTTP_SPX_ENABLED").count(), 1, "only the profiled site: {file}");
}

/// SPX's report pages fetch their scripts and data without the key: every site whose PHP has SPX
/// adds it to those requests, also when the site is not profiled.
#[test]
fn a_site_on_a_php_with_spx_gives_its_report_pages_the_key() {
    let route = |name: &str, spx_ui: Option<String>| SiteRoute {
        name: name.into(),
        tls: None,
        target: Route::Php {
            docroot: PathBuf::from(format!("/srv/{name}")),
            minor: "8.4".into(),
            xdebug: false,
            spx: None,
            spx_ui,
        },
    };
    let file = render(&ctx(vec![route("shop", Some("k123".into())), route("blog", None)]));
    assert!(
        file.contains("\troot * \"/srv/shop\"\n\t@spxui expression {query.SPX_UI_URI} != \"\"\n\trequest_header @spxui SPX-Key k123\n\tphp_fastcgi "),
        "{file}"
    );
    assert_eq!(file.matches("@spxui expression").count(), 1, "only the site on a PHP with SPX: {file}");
}
