//! Names derived from a branch: the host label, the database name, a random branch.

/// The longest host Hearth gives a workspace, `.test` included.
const MAX_HOST: usize = 60;
const TLD: &str = ".test";
const MAX_DATABASE: usize = 63;

/// `text` lowercased, every character outside `keep` turned into `sep`, runs collapsed, trimmed.
fn sanitize(text: &str, sep: char, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = if keep(c) { c } else { sep };
        if c == sep && (out.is_empty() || out.ends_with(sep)) {
            continue;
        }
        out.push(c);
    }
    out.trim_end_matches(sep).to_string()
}

/// The host label for a branch: `feature/Login` -> `feature-login`.
pub fn label(branch: &str) -> String {
    let label = sanitize(branch, '-', |c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if label.is_empty() {
        "workspace".into()
    } else {
        label
    }
}

/// `base` cut to `max` characters (ASCII here), with a trailing `sep` removed.
fn cut(base: &str, max: usize, sep: char) -> String {
    base.chars().take(max).collect::<String>().trim_end_matches(sep).to_string()
}

/// `<label>.<parent>`: a subdomain of the parent site (cookies for `.parent.test` keep working),
/// short enough for a host, and not `taken` (`-2`, `-3`, ... otherwise).
pub fn host(label: &str, parent: &str, taken: impl Fn(&str) -> bool) -> String {
    let budget = MAX_HOST.saturating_sub(TLD.len() + 1 + parent.len()).max(8);
    let base = cut(label, budget, '-');
    let mut candidate = format!("{base}.{parent}");
    let mut n = 2;
    while taken(&candidate) {
        let suffix = format!("-{n}");
        candidate = format!("{}{suffix}.{parent}", cut(&base, budget.saturating_sub(suffix.len()), '-'));
        n += 1;
    }
    candidate
}

/// `<parent db>_<branch>`, lowercase with `_`, at most 63 characters, not `taken` (`_2`, ...).
pub fn database(parent_db: &str, branch: &str, taken: impl Fn(&str) -> bool) -> String {
    let keep = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
    let base = cut(&sanitize(&format!("{parent_db}_{branch}"), '_', keep), MAX_DATABASE, '_');
    let mut candidate = base.clone();
    let mut n = 2;
    while taken(&candidate) || candidate == parent_db {
        let suffix = format!("_{n}");
        candidate = format!("{}{suffix}", cut(&base, MAX_DATABASE - suffix.len(), '_'));
        n += 1;
    }
    candidate
}

const ADJECTIVES: [&str; 16] = [
    "brave", "calm", "clever", "eager", "gentle", "happy", "jolly", "kind", "lively", "lucky", "merry", "proud",
    "quick", "quiet", "swift", "witty",
];
const ANIMALS: [&str; 16] = [
    "badger", "beaver", "crane", "falcon", "fox", "heron", "lynx", "marten", "otter", "owl", "panda", "puffin",
    "raven", "seal", "tiger", "wren",
];

/// A random unused `<adjective>-<animal>` branch name (a number is added when all are used).
pub fn random_branch(taken: impl Fn(&str) -> bool) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u8(0);
    let start = h.finish() as usize;
    let total = ADJECTIVES.len() * ANIMALS.len();
    for i in 0..total {
        let k = (start + i) % total;
        let name = format!("{}-{}", ADJECTIVES[k / ANIMALS.len()], ANIMALS[k % ANIMALS.len()]);
        if !taken(&name) {
            return name;
        }
    }
    let k = start % total;
    let base = format!("{}-{}", ADJECTIVES[k / ANIMALS.len()], ANIMALS[k % ANIMALS.len()]);
    (2..).map(|n| format!("{base}{n}")).find(|n| !taken(n)).unwrap_or(base)
}
