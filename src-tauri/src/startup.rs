use hearth_core::Settings;

/// Passed by the "Start at login" entry, so a login start goes straight to the tray.
pub const HIDDEN_FLAG: &str = "--hidden";

/// The name of the login entry (Windows Run value, macOS LaunchAgent); the uninstaller removes it by
/// this name (windows/hooks.nsh).
pub const LOGIN_ENTRY_NAME: &str = "Hearth";

/// Installed Hearth starts with the OS by default: on the first run of a release build, "Start at
/// login" is switched on. Later runs keep whatever the user chose; dev builds never register.
pub fn first_run_settings(first_run: bool, release_build: bool, current: &Settings) -> Option<Settings> {
    (first_run && release_build).then(|| Settings { launch_at_login: true, ..current.clone() })
}

/// The app-wide single-instance lock is keyed on the app identifier only, so it applies to the
/// default home alone; a custom `HEARTH_HOME` (dev or test build) relies on the core's per-home lock.
pub fn use_single_instance(hearth_home: Option<&std::ffi::OsStr>) -> bool {
    hearth_home.is_none_or(|home| home.is_empty())
}

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
    fn single_instance_for_the_default_home() {
        assert!(use_single_instance(None));
        assert!(use_single_instance(Some(std::ffi::OsStr::new(""))));
    }

    #[test]
    fn no_single_instance_for_a_custom_home() {
        // A dev build with its own HEARTH_HOME runs next to the installed app; the per-home IPC lock
        // still stops a second core for the same home.
        assert!(!use_single_instance(Some(std::ffi::OsStr::new("C:/dev/hearth-home"))));
    }

    #[test]
    fn first_run_of_release_enables_login_start() {
        let current = Settings { theme: hearth_core::Theme::Dark, ..Settings::default() };
        let next = first_run_settings(true, true, &current).unwrap();
        assert!(next.launch_at_login);
        assert_eq!(next.theme, hearth_core::Theme::Dark);
    }

    #[test]
    fn debug_build_never_enables_it() {
        assert_eq!(first_run_settings(true, false, &Settings::default()), None);
    }

    #[test]
    fn later_runs_keep_the_users_choice() {
        assert_eq!(first_run_settings(false, true, &Settings::default()), None);
    }

    #[test]
    fn visible_by_default() {
        assert!(!start_hidden(&Settings::default(), &args(&["hearth-app"])));
    }
}
