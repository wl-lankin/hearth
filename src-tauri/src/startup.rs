use hearth_core::Settings;

/// Passed by the "Start at login" entry, so a login start goes straight to the tray.
pub const HIDDEN_FLAG: &str = "--hidden";

pub fn start_hidden(settings: &Settings, args: &[String]) -> bool {
    settings.start_hidden || args.iter().any(|a| a == HIDDEN_FLAG)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hearth_core::Settings;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn start_hidden_from_setting() {
        let settings = Settings { start_hidden: true, ..Settings::default() };
        assert!(start_hidden(&settings, &args(&["hearth-app"])));
    }

    #[test]
    fn start_hidden_from_flag() {
        assert!(start_hidden(&Settings::default(), &args(&["hearth-app", HIDDEN_FLAG])));
    }

    #[test]
    fn visible_by_default() {
        assert!(!start_hidden(&Settings::default(), &args(&["hearth-app"])));
    }
}
