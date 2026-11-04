//! The tray panel: a small Ember window at the tray icon (right-click), with quick actions.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, PhysicalPosition};

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Space between the tray icon and the panel.
const GAP: f64 = 8.0;

/// The transparent margin around the panel's card, room for its shadow (logical pixels; the page
/// uses the same value).
pub const MARGIN: f64 = 24.0;

/// The top-left corner of a `window_w` x `window_h` panel window at the tray `icon`. The card inside
/// (the window less `margin` on each side) goes above the icon when the icon is in the lower half
/// of the screen (a Windows taskbar), else below it (the macOS menu bar), its right edge at the
/// icon's. The whole window, margin included, stays inside the work area: its invisible part must
/// not cover the taskbar, where it would catch the clicks meant for the tray icons and the clock.
pub fn place(icon: Area, window_w: f64, window_h: f64, work: Area, margin: f64) -> (f64, f64) {
    let h = window_h - 2.0 * margin;
    let below = icon.y + icon.h / 2.0 < work.y + work.h / 2.0;
    let x = icon.x + icon.w - window_w + margin;
    let y = if below { icon.y + icon.h + GAP - margin } else { icon.y - GAP - h - margin };
    // min before max: a window larger than the screen starts at its top left.
    let x = x.min(work.x + work.w - window_w).max(work.x);
    let y = y.min(work.y + work.h - window_h).max(work.y);
    (x, y)
}

/// Which of `screens` holds `point` (all in the same units).
pub fn containing(point: (f64, f64), screens: &[Area]) -> Option<usize> {
    screens.iter().position(|s| point.0 >= s.x && point.0 < s.x + s.w && point.1 >= s.y && point.1 < s.y + s.h)
}

fn scaled(a: Area, by: f64) -> Area {
    Area { x: a.x / by, y: a.y / by, w: a.w / by, h: a.h / by }
}

pub const LABEL: &str = "panel";

/// Where the tray icon was when the panel last opened, and when the panel last lost focus.
#[derive(Default)]
pub struct PanelState(Mutex<(Option<Area>, Option<Instant>)>);

/// A right-click on the icon right after the panel closed is the click that closed it: no reopen.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// The screen with the tray icon, found by each screen's own physical rectangle (macOS'
/// `monitor_from_point` compares points instead), else the primary one.
fn screen_of(app: &AppHandle, icon: Area) -> Option<tauri::Monitor> {
    let monitors = app.available_monitors().ok().unwrap_or_default();
    let rects: Vec<Area> = monitors
        .iter()
        .map(|m| Area {
            x: f64::from(m.position().x),
            y: f64::from(m.position().y),
            w: f64::from(m.size().width),
            h: f64::from(m.size().height),
        })
        .collect();
    match containing((icon.x + icon.w / 2.0, icon.y + icon.h / 2.0), &rects) {
        Some(i) => monitors.into_iter().nth(i),
        None => app.primary_monitor().ok().flatten(),
    }
}

/// Places the panel at `icon` (physical pixels of its screen) with the window's current size.
fn position(app: &AppHandle, window: &tauri::WebviewWindow, icon: Area) {
    let (Some(screen), Ok(size)) = (screen_of(app, icon), window.outer_size()) else { return };
    let area = screen.work_area();
    let work = Area {
        x: f64::from(area.position.x),
        y: f64::from(area.position.y),
        w: f64::from(area.size.width),
        h: f64::from(area.size.height),
    };
    let (w, h) = (f64::from(size.width), f64::from(size.height));
    if cfg!(target_os = "macos") {
        // macOS positions windows in points, and a Mac's screens can have different scales: do the
        // math in points of the icon's screen.
        let (screen_scale, window_scale) = (screen.scale_factor(), window.scale_factor().unwrap_or(1.0));
        let (x, y) =
            place(scaled(icon, screen_scale), w / window_scale, h / window_scale, scaled(work, screen_scale), MARGIN);
        let _ = window.set_position(LogicalPosition::new(x, y));
    } else {
        let (x, y) = place(icon, w, h, work, MARGIN * screen.scale_factor());
        let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
    }
}

/// Opens the panel at the tray icon, or closes it when it is open.
pub fn toggle(app: &AppHandle, rect: tauri::Rect) {
    let Some(window) = app.get_webview_window(LABEL) else { return };
    let state = app.state::<PanelState>();
    let mut guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    if guard.1.is_some_and(|closed| closed.elapsed() < REOPEN_GUARD) {
        return;
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    let at = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    let icon = Area { x: at.x, y: at.y, w: size.width, h: size.height };
    guard.0 = Some(icon);
    drop(guard);
    position(app, &window, icon);
    let _ = window.show();
    let _ = window.set_focus();
}

/// Resizes the panel to what its page measured (logical pixels) and keeps it at the icon.
pub fn fit(app: &AppHandle, width: f64, height: f64) {
    let Some(window) = app.get_webview_window(LABEL) else { return };
    let _ = window.set_size(LogicalSize::new(width, height));
    let icon = app.state::<PanelState>().0.lock().unwrap_or_else(|e| e.into_inner()).0;
    if let Some(icon) = icon {
        position(app, &window, icon);
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

/// The panel lost focus (a click elsewhere): it closes.
pub fn blurred(app: &AppHandle) {
    hide(app);
    app.state::<PanelState>().0.lock().unwrap_or_else(|e| e.into_inner()).1 = Some(Instant::now());
}

#[cfg(test)]
mod tests {
    use super::{containing, place, Area};

    const SCREEN: Area = Area { x: 0.0, y: 0.0, w: 1920.0, h: 1040.0 };

    #[test]
    fn a_windows_taskbar_icon_opens_the_panel_above_it() {
        let icon = Area { x: 1800.0, y: 1044.0, w: 24.0, h: 36.0 };
        assert_eq!(place(icon, 300.0, 400.0, SCREEN, 0.0), (1524.0, 636.0));
    }

    #[test]
    fn the_whole_window_with_its_shadow_margin_stays_off_the_taskbar() {
        let icon = Area { x: 1800.0, y: 1044.0, w: 24.0, h: 36.0 };
        // A 300x400 card in a window 32 px bigger on each side: the window ends at the taskbar, so
        // nothing invisible covers the tray icons or the clock.
        let (x, y) = place(icon, 364.0, 464.0, SCREEN, 32.0);
        assert_eq!((x, y), (1492.0, 576.0));
        assert!(y + 464.0 <= SCREEN.y + SCREEN.h);
        let corner = Area { x: 1900.0, y: 1044.0, w: 24.0, h: 36.0 };
        let (x, _) = place(corner, 364.0, 464.0, SCREEN, 32.0);
        assert_eq!(x + 364.0, 1920.0, "the window stops at the screen edge");
        let mac = Area { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 };
        let (_, y) = place(Area { x: 1300.0, y: 0.0, w: 24.0, h: 24.0 }, 364.0, 464.0, mac, 32.0);
        assert_eq!(y, 25.0, "not over the menu bar either");
    }

    #[test]
    fn the_icon_belongs_to_the_screen_that_contains_it() {
        let screens = [Area { x: 0.0, y: 0.0, w: 2880.0, h: 1800.0 }, Area { x: 2880.0, y: 0.0, w: 1920.0, h: 1080.0 }];
        assert_eq!(containing((100.0, 10.0), &screens), Some(0));
        assert_eq!(containing((3000.0, 10.0), &screens), Some(1));
        assert_eq!(containing((9000.0, 10.0), &screens), None);
    }

    #[test]
    fn a_macos_menu_bar_icon_opens_the_panel_below_it() {
        let work = Area { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 };
        let icon = Area { x: 1500.0, y: 0.0, w: 24.0, h: 24.0 };
        let (x, y) = place(icon, 300.0, 400.0, work, 0.0);
        assert_eq!(x, 1140.0, "clamped inside the 1440 wide screen");
        assert_eq!(y, 32.0, "8 px below the icon");
    }

    #[test]
    fn the_panel_stays_on_screen() {
        let left = Area { x: 10.0, y: 1044.0, w: 24.0, h: 36.0 };
        assert_eq!(place(left, 300.0, 400.0, SCREEN, 0.0).0, 0.0);

        let second = Area { x: 1920.0, y: 0.0, w: 2560.0, h: 1400.0 };
        let icon = Area { x: 4400.0, y: 1404.0, w: 24.0, h: 36.0 };
        let (x, y) = place(icon, 300.0, 400.0, second, 0.0);
        assert!((1920.0..=4480.0 - 300.0).contains(&x), "{x}");
        assert_eq!(y, 1404.0 - 8.0 - 400.0);

        let (_, y) = place(Area { x: 1800.0, y: 1044.0, w: 24.0, h: 36.0 }, 300.0, 2000.0, SCREEN, 0.0);
        assert_eq!(y, 0.0, "too tall: starts at the top of the work area");
    }
}
