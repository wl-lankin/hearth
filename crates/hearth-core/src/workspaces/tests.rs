use crate::workspaces::env_file;
use crate::workspaces::names;

#[test]
fn labels_come_from_the_branch_and_stay_valid_host_parts() {
    assert_eq!(names::label("feature/Ölrechnung"), "feature-lrechnung");
    assert_eq!(names::label("Fix__Login  Page"), "fix-login-page");
    assert_eq!(names::label("--x--"), "x");
    assert_eq!(names::label(""), "workspace");
    assert_eq!(names::label("///"), "workspace");
}

#[test]
fn hosts_are_subdomains_of_the_parent_kept_short_and_unique() {
    let none = |_: &str| false;
    assert_eq!(names::host("feature-x", "shop", none), "feature-x.shop");
    let long = "a".repeat(80);
    let host = names::host(&long, "shop", none);
    assert!(host.len() + ".test".len() <= 60, "{host}");
    assert!(host.ends_with(".shop") && !host.contains("-."), "{host}");
    let taken = |h: &str| h == "feature-x.shop" || h == "feature-x-2.shop";
    assert_eq!(names::host("feature-x", "shop", taken), "feature-x-3.shop");
}

#[test]
fn database_names_join_parent_and_branch() {
    let none = |_: &str| false;
    assert_eq!(names::database("shop", "feature/X-1", none), "shop_feature_x_1");
    assert!(names::database(&"d".repeat(50), &"b".repeat(50), none).len() <= 63);
    let taken = |n: &str| n == "shop_x";
    assert_eq!(names::database("shop", "x", taken), "shop_x_2");
}

#[test]
fn a_random_branch_is_an_adjective_and_an_animal_and_unused() {
    let b = names::random_branch(|_| false);
    let parts: Vec<&str> = b.split('-').collect();
    assert_eq!(parts.len(), 2, "{b}");
    assert!(b.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{b}");
    let other = names::random_branch(|n| n == b);
    assert_ne!(other, b);
}

#[test]
fn env_edits_keep_line_endings_comments_and_unknown_lines() {
    let text = "# app\r\nAPP_NAME=Shop\r\nexport APP_URL=http://old\r\nAPP_URL=http://shop.test\r\nAPP_URL=http://second\r\n\r\nDB_HOST=127.0.0.1\r\n";
    let out =
        env_file::set(text, &[("APP_URL", "http://x.shop.test"), ("DB_DATABASE", "shop_x"), ("APP_NAME", "My Shop")]);
    assert_eq!(
        out,
        "# app\r\nAPP_NAME=\"My Shop\"\r\nexport APP_URL=http://old\r\nAPP_URL=http://x.shop.test\r\nAPP_URL=http://second\r\n\r\nDB_HOST=127.0.0.1\r\nDB_DATABASE=shop_x\r\n"
    );
    let lf = env_file::set("A=1\n", &[("B", "say \"hi\" $X #1")]);
    assert_eq!(lf, "A=1\nB=\"say \\\"hi\\\" $X #1\"\n");
    assert_eq!(env_file::set("", &[("A", "1")]), "A=1\n");
}

#[test]
fn env_values_are_read_without_quotes_or_comments() {
    let text =
        "# DB_DATABASE=nope\nDB_DATABASE=\"shop db\" # main\nexport DB_HOST=localhost\nDB_PORT=3307 # local\nEMPTY=\n";
    assert_eq!(env_file::get(text, "DB_DATABASE").as_deref(), Some("shop db"));
    assert_eq!(env_file::get(text, "DB_HOST").as_deref(), Some("localhost"));
    assert_eq!(env_file::get(text, "DB_PORT").as_deref(), Some("3307"));
    assert_eq!(env_file::get(text, "EMPTY").as_deref(), Some(""));
    assert_eq!(env_file::get(text, "MISSING"), None);
}
