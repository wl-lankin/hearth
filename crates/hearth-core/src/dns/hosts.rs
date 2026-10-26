//! Hearth's block in the hosts file. Everything outside the block is kept byte for byte.

use std::collections::BTreeSet;

pub const START: &str = "# hearth-start";
pub const END: &str = "# hearth-end";

/// A full host name Hearth may write: site-name labels plus `.test`, at most 253 characters.
pub fn valid_host(host: &str) -> bool {
    host.len() <= 253 && host.strip_suffix(".test").is_some_and(crate::sites::registry::valid_name)
}

/// Byte ranges of the file's lines, each including its line break.
fn lines(hosts: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, b) in hosts.iter().enumerate() {
        if *b == b'\n' {
            out.push((start, i + 1));
            start = i + 1;
        }
    }
    if start < hosts.len() {
        out.push((start, hosts.len()));
    }
    out
}

fn is_marker(line: &[u8], marker: &str) -> bool {
    String::from_utf8_lossy(line).trim_start_matches('\u{feff}').trim() == marker
}

/// A line Hearth itself writes inside its block.
fn is_hearth_line(line: &[u8]) -> bool {
    let text = String::from_utf8_lossy(line);
    let mut parts = text.split_whitespace();
    matches!((parts.next(), parts.next(), parts.next()), (Some("127.0.0.1"), Some(host), None) if valid_host(host))
}

/// Where Hearth's block sits: from its start line to the end of its end line. A block whose end
/// marker is missing ends before the first line Hearth would not have written, so nothing of
/// anyone else's is ever removed.
fn block(hosts: &[u8]) -> Option<(usize, usize)> {
    let all = lines(hosts);
    let first = all.iter().position(|&(s, e)| is_marker(&hosts[s..e], START))?;
    let mut last = all[first].1;
    for &(s, e) in &all[first + 1..] {
        if is_marker(&hosts[s..e], END) {
            return Some((all[first].0, e));
        }
        if !is_hearth_line(&hosts[s..e]) {
            break;
        }
        last = e;
    }
    Some((all[first].0, last))
}

/// `hosts` with Hearth's block holding exactly `names` (removed when `names` is empty).
pub fn apply_block(hosts: &[u8], names: &BTreeSet<String>) -> Vec<u8> {
    // The file's own style (Windows reads LF lines too).
    let eol: &[u8] = if hosts.windows(2).any(|w| w == b"\r\n") { b"\r\n" } else { b"\n" };
    let mut text = Vec::new();
    if !names.is_empty() {
        text.extend_from_slice(START.as_bytes());
        text.extend_from_slice(eol);
        for name in names {
            text.extend_from_slice(format!("127.0.0.1 {name}").as_bytes());
            text.extend_from_slice(eol);
        }
        text.extend_from_slice(END.as_bytes());
        text.extend_from_slice(eol);
    }
    let mut out = Vec::with_capacity(hosts.len() + text.len());
    match block(hosts) {
        Some((start, end)) => {
            out.extend_from_slice(&hosts[..start]);
            out.extend_from_slice(&text);
            out.extend_from_slice(&hosts[end..]);
        }
        None => {
            out.extend_from_slice(hosts);
            if !text.is_empty() && !hosts.is_empty() && !hosts.ends_with(b"\n") {
                out.extend_from_slice(eol);
            }
            out.extend_from_slice(&text);
        }
    }
    out
}

/// The names in Hearth's block.
pub fn names_in_block(hosts: &[u8]) -> BTreeSet<String> {
    let Some((start, end)) = block(hosts) else { return BTreeSet::new() };
    String::from_utf8_lossy(&hosts[start..end])
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    const HERD: &str = "#herd\r\n127.0.0.1 shop.test\r\n#/herd\r\n";

    #[test]
    fn a_block_is_appended_in_the_files_line_ending_style() {
        let crlf = format!("127.0.0.1 localhost\r\n{HERD}");
        let out = apply_block(crlf.as_bytes(), &names(&["b.test", "a.test"]));
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("{crlf}# hearth-start\r\n127.0.0.1 a.test\r\n127.0.0.1 b.test\r\n# hearth-end\r\n")
        );
        let lf = "127.0.0.1 localhost\n";
        let out = apply_block(lf.as_bytes(), &names(&["a.test"]));
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "127.0.0.1 localhost\n# hearth-start\n127.0.0.1 a.test\n# hearth-end\n"
        );
    }

    #[test]
    fn a_missing_trailing_newline_is_added_before_the_block() {
        let out = apply_block(b"127.0.0.1 localhost", &names(&["a.test"]));
        assert_eq!(out, b"127.0.0.1 localhost\n# hearth-start\n127.0.0.1 a.test\n# hearth-end\n");
    }

    #[test]
    fn a_bom_and_everything_around_the_block_stay_byte_identical() {
        let before = format!("\u{feff}# head\r\n{START}\r\n127.0.0.1 old.test\r\n{END}\r\n{HERD}# tail");
        let out = String::from_utf8(apply_block(before.as_bytes(), &names(&["new.test"]))).unwrap();
        assert_eq!(out, format!("\u{feff}# head\r\n{START}\r\n127.0.0.1 new.test\r\n{END}\r\n{HERD}# tail"));
    }

    #[test]
    fn a_second_apply_changes_nothing() {
        let once = apply_block(HERD.as_bytes(), &names(&["a.test", "b.test"]));
        assert_eq!(apply_block(&once, &names(&["a.test", "b.test"])), once);
    }

    #[test]
    fn no_names_removes_the_block() {
        let with = apply_block(HERD.as_bytes(), &names(&["a.test"]));
        assert_eq!(apply_block(&with, &BTreeSet::new()), HERD.as_bytes());
        assert_eq!(apply_block(HERD.as_bytes(), &BTreeSet::new()), HERD.as_bytes());
    }

    #[test]
    fn a_block_without_its_end_stops_before_foreign_lines() {
        let after = "# Added by Docker Desktop\r\n10.0.0.5 host.docker.internal\r\n";
        let broken = format!("{START}\r\n127.0.0.1 half.test\r\n{after}");
        let out = String::from_utf8(apply_block(broken.as_bytes(), &names(&["a.test"]))).unwrap();
        assert_eq!(out, format!("{START}\r\n127.0.0.1 a.test\r\n{END}\r\n{after}"));
    }

    #[test]
    fn a_block_without_its_end_is_replaced_to_the_end_of_the_file() {
        let broken = format!("{HERD}{START}\r\n127.0.0.1 half.test\r\n");
        let out = String::from_utf8(apply_block(broken.as_bytes(), &names(&["a.test"]))).unwrap();
        assert_eq!(out, format!("{HERD}{START}\r\n127.0.0.1 a.test\r\n{END}\r\n"));
        assert_eq!(out.matches(START).count(), 1);
    }

    #[test]
    fn the_names_in_the_block_are_read_back() {
        let with = apply_block(HERD.as_bytes(), &names(&["a.test", "b.test"]));
        assert_eq!(names_in_block(&with), names(&["a.test", "b.test"]));
        assert!(names_in_block(HERD.as_bytes()).is_empty(), "Herd's names are not Hearth's");
    }

    #[test]
    fn only_test_site_names_are_valid_hosts() {
        for ok in ["shop.test", "api.shop.test", "a-b.test"] {
            assert!(valid_host(ok), "{ok}");
        }
        let long = format!("{}.test", vec!["a".repeat(63); 4].join("."));
        for bad in
            ["a.test\n1.2.3.4 bank.com", "bank.com", "test", ".test", "Shop.test", "-a.test", "a b.test", long.as_str()]
        {
            assert!(!valid_host(bad), "{bad:?}");
        }
    }
}
