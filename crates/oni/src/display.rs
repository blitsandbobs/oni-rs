//! The window (#97): windowed at a kept size or fullscreen at the
//! monitor's native resolution (borderless — no display mode switching),
//! and the scale the 2D screens are drawn at.
//!
//! The original ran at a chosen display mode, its HUD and menus pixel for
//! pixel; here the window is the host's, and the overlays scale with it.
//!
//! Sizes are the desktop's own units — points on macOS, scaled pixels on
//! Windows, Wayland and X11 with display scaling (`scale`: physical pixels
//! a unit) — as every other window there; the frame is drawn at every
//! physical pixel. (Wayland has no primary monitor: the first one.)

use winit::dpi::LogicalSize;
use winit::monitor::MonitorHandle;
use winit::window::{Fullscreen, Window, WindowAttributes};

/// How the game is shown: kept in the settings, overridden by the command
/// line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Display {
    pub fullscreen: bool,
    /// The window's size in the desktop's units (none: from the monitor).
    pub window: Option<(u32, u32)>,
    /// A fixed scale for the 2D screens (none: from the window's height).
    pub ui_scale: Option<f32>,
}

/// The command line's say: `--fullscreen`, `--windowed`, `--window WxH`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Overrides {
    pub fullscreen: Option<bool>,
    pub window: Option<(u32, u32)>,
}

impl Overrides {
    /// Takes the display flags out of `args`, leaving the rest in order.
    pub fn take(args: &mut Vec<String>) -> Overrides {
        let mut o = Overrides::default();
        let mut rest = Vec::with_capacity(args.len());
        let mut it = std::mem::take(args).into_iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--fullscreen" => o.fullscreen = Some(true),
                "--windowed" => o.fullscreen = Some(false),
                "--window" => o.window = it.next().as_deref().and_then(parse_size),
                _ => rest.push(a),
            }
        }
        *args = rest;
        o
    }

    pub fn apply(self, d: &mut Display) {
        if let Some(f) = self.fullscreen {
            d.fullscreen = f;
        }
        if let Some(w) = self.window {
            d.window = Some(w);
            // (A size asked for is a window's.)
            d.fullscreen = self.fullscreen.unwrap_or(false);
        }
    }
}

/// `1920x1080` → (1920, 1080).
pub fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w >= 320 && h >= 240).then_some((w, h))
}

/// The 2D screens' scale (physical pixels a layout pixel) for a window
/// `height` physical pixels high on a display of `scale` pixels a unit:
/// the setting's, else a step of a half for each 360 units past 720 —
/// 1080 1.5, 1440 2, 2160 3 — never below 1; either times the display's
/// scale. (So a display shows them as big as an unscaled one of as many
/// units: a Retina laptop 2, a 4K monitor 3 at 100% or 150%.)
pub fn ui_scale(setting: Option<f32>, height: u32, scale: f64) -> f32 {
    let scale = if scale > 0.0 { scale as f32 } else { 1.0 };
    let steps = setting
        .filter(|s| *s > 0.0)
        .unwrap_or_else(|| ((height as f32 / scale / 720.0) * 2.0).floor().max(2.0) / 2.0);
    steps * scale
}

/// A window `want` (else three quarters of the monitor) kept inside the
/// monitor (`monitor`: its size), its shape kept.
pub fn fit_window(want: Option<(u32, u32)>, monitor: Option<(u32, u32)>) -> (u32, u32) {
    let Some((mw, mh)) = monitor.filter(|(w, h)| *w > 0 && *h > 0) else {
        return want.unwrap_or((1280, 720));
    };
    let (w, h) = want.unwrap_or((mw * 3 / 4, mh * 3 / 4));
    // (Room for the title bar and the desktop's own bars.)
    let (lw, lh) = (mw as f32 * 0.95, mh as f32 * 0.9);
    let s = (lw / w as f32).min(lh / h as f32).min(1.0);
    (
        ((w as f32 * s) as u32).max(320),
        ((h as f32 * s) as u32).max(240),
    )
}

/// `monitor`'s size in the desktop's units.
fn monitor_size(m: Option<&MonitorHandle>) -> Option<(u32, u32)> {
    m.map(|m| {
        let s: LogicalSize<u32> = m.size().to_logical(m.scale_factor());
        (s.width, s.height)
    })
}

/// The usual window sizes (the original's first, then today's), in the
/// desktop's units.
const SIZES: [(u32, u32); 17] = [
    (640, 480),
    (800, 600),
    (1024, 768),
    (1280, 720),
    (1280, 800),
    (1366, 768),
    (1440, 900),
    (1600, 900),
    (1680, 1050),
    (1920, 1080),
    (1920, 1200),
    (2560, 1080),
    (2560, 1440),
    (2560, 1600),
    (3200, 1800),
    (3440, 1440),
    (3840, 2160),
];

/// The window sizes offered on `monitor`: the usual ones that fit it, and
/// its own size (smallest first).
pub fn resolutions(monitor: Option<&MonitorHandle>) -> Vec<(u32, u32)> {
    monitor_size(monitor).map(sizes).unwrap_or_default()
}

fn sizes((mw, mh): (u32, u32)) -> Vec<(u32, u32)> {
    let mut list: Vec<(u32, u32)> = SIZES
        .into_iter()
        .chain(std::iter::once((mw, mh)))
        .filter(|&(w, h)| w <= mw && h <= mh)
        .collect();
    list.sort_by_key(|&(w, h)| (w, h));
    list.dedup();
    list
}

/// The monitor to open on: the primary, else (Wayland has none) the first.
pub fn first_monitor(event_loop: &winit::event_loop::ActiveEventLoop) -> Option<MonitorHandle> {
    event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next())
}

/// The window's attributes as `d` has it, on `monitor`.
pub fn attributes(d: &Display, monitor: Option<MonitorHandle>) -> WindowAttributes {
    let (w, h) = fit_window(d.window, monitor_size(monitor.as_ref()));
    let mut a = Window::default_attributes()
        .with_title("Oni")
        .with_inner_size(LogicalSize::new(w, h));
    if d.fullscreen {
        a = a.with_fullscreen(Some(Fullscreen::Borderless(monitor)));
    }
    a
}

/// Fullscreen on or off for `window` (on: borderless on the monitor it is
/// on).
pub fn set_fullscreen(window: &Window, on: bool) {
    window.set_fullscreen(on.then(|| Fullscreen::Borderless(window.current_monitor())));
}

/// Keeps `d` in the settings file, the other options as they are.
pub fn keep(d: &Display) {
    let mut s = crate::settings::Settings::load();
    s.set_display(d);
    s.store();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ui_scales_in_half_steps_from_720_units_times_the_displays() {
        assert_eq!(ui_scale(None, 480, 1.0), 1.0);
        assert_eq!(ui_scale(None, 720, 1.0), 1.0);
        assert_eq!(ui_scale(None, 1080, 1.0), 1.5);
        assert_eq!(ui_scale(None, 1440, 1.0), 2.0);
        assert_eq!(ui_scale(None, 2160, 1.0), 3.0, "4K at 100%");
        assert_eq!(ui_scale(None, 2160, 1.5), 3.0, "4K at 150%: 1440 units");
        assert_eq!(
            ui_scale(None, 1964, 2.0),
            2.0,
            "a Retina laptop: 982 points"
        );
        assert_eq!(ui_scale(None, 2880, 2.0), 4.0, "a 5K iMac: 1440 points");
        assert_eq!(
            ui_scale(None, 1350, 1.25),
            1.875,
            "1080 units at 125%: 1.5 × 1.25"
        );
        assert_eq!(ui_scale(Some(1.0), 2160, 1.0), 1.0, "fixed: as set");
        assert_eq!(
            ui_scale(Some(1.0), 1964, 2.0),
            2.0,
            "…in the display's units"
        );
    }

    #[test]
    fn a_window_fits_its_monitor() {
        assert_eq!(fit_window(None, Some((3840, 2160))), (2880, 1620));
        assert_eq!(
            fit_window(Some((1280, 720)), Some((3840, 2160))),
            (1280, 720)
        );
        let (w, h) = fit_window(Some((3840, 2160)), Some((1920, 1080)));
        assert!(w <= 1824 && h <= 972 && (w as f32 / h as f32 - 16.0 / 9.0).abs() < 0.01);
        assert_eq!(fit_window(None, None), (1280, 720));
    }

    #[test]
    fn the_display_offers_the_sizes_that_fit_it_and_its_own() {
        let laptop = sizes((1512, 982));
        assert_eq!(laptop.first(), Some(&(640, 480)));
        assert_eq!(laptop.last(), Some(&(1512, 982)), "its own size last");
        assert!(laptop.contains(&(1440, 900)) && !laptop.contains(&(1600, 900)));
        let uhd = sizes((3840, 2160));
        assert_eq!(uhd.last(), Some(&(3840, 2160)), "once");
        assert_eq!(uhd.iter().filter(|s| **s == (3840, 2160)).count(), 1);
        assert!(uhd.contains(&(3440, 1440)), "an ultrawide fits 3840 across");
    }

    #[test]
    fn the_command_line_says_how() {
        let mut args: Vec<String> = ["lvl.glb", "--window", "1920x1080", "--play"]
            .map(String::from)
            .to_vec();
        let o = Overrides::take(&mut args);
        assert_eq!(args, ["lvl.glb", "--play"]);
        let mut d = Display {
            fullscreen: true,
            ..Display::default()
        };
        o.apply(&mut d);
        assert_eq!(
            d,
            Display {
                fullscreen: false,
                window: Some((1920, 1080)),
                ui_scale: None
            }
        );
        let mut args = vec!["--fullscreen".to_string()];
        Overrides::take(&mut args).apply(&mut d);
        assert!(d.fullscreen && args.is_empty());
        assert_eq!(parse_size("10x10"), None);
    }
}
