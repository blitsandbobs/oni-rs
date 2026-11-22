//! The window (#97): windowed at a kept size or fullscreen at the
//! monitor's native resolution (borderless — no display mode switching),
//! and the scale the 2D screens are drawn at.
//!
//! The original ran at a chosen display mode, its HUD and menus pixel for
//! pixel; here the window is the host's, and the overlays scale with it.

use winit::dpi::PhysicalSize;
use winit::monitor::MonitorHandle;
use winit::window::{Fullscreen, Window, WindowAttributes};

/// How the game is shown: kept in the settings, overridden by the command
/// line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Display {
    pub fullscreen: bool,
    /// The window's size in physical pixels (none: from the monitor).
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

/// The 2D screens' scale for a window `height` physical pixels high: the
/// setting's, else a step of a half for each 360 lines past 720 — 1080p
/// 1.5, 1440p 2, 4K 3 (a Retina laptop about 2.5) — never below 1.
pub fn ui_scale(setting: Option<f32>, height: u32) -> f32 {
    if let Some(s) = setting.filter(|s| *s > 0.0) {
        return s;
    }
    ((height as f32 / 720.0) * 2.0).floor().max(2.0) / 2.0
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

fn monitor_size(m: Option<&MonitorHandle>) -> Option<(u32, u32)> {
    m.map(|m| {
        let s = m.size();
        (s.width, s.height)
    })
}

/// The window sizes `monitor` offers (its display modes' sizes, at least
/// 640 × 480 and no larger than it, smallest first; its own size last).
pub fn resolutions(monitor: Option<&MonitorHandle>) -> Vec<(u32, u32)> {
    let Some(m) = monitor else { return Vec::new() };
    let (mw, mh) = (m.size().width, m.size().height);
    let modes = m.video_modes().map(|v| (v.size().width, v.size().height));
    sizes(modes, (mw, mh))
}

fn sizes(modes: impl Iterator<Item = (u32, u32)>, (mw, mh): (u32, u32)) -> Vec<(u32, u32)> {
    let mut list: Vec<(u32, u32)> = modes
        .chain(std::iter::once((mw, mh)))
        .filter(|&(w, h)| w >= 640 && h >= 480 && w <= mw && h <= mh)
        .collect();
    list.sort_by_key(|&(w, h)| (w, h));
    list.dedup();
    list
}

/// The window's attributes as `d` has it, on `monitor`.
pub fn attributes(d: &Display, monitor: Option<MonitorHandle>) -> WindowAttributes {
    let (w, h) = fit_window(d.window, monitor_size(monitor.as_ref()));
    let mut a = Window::default_attributes()
        .with_title("Oni")
        .with_inner_size(PhysicalSize::new(w, h));
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
    fn the_ui_scales_in_half_steps_from_720_lines() {
        assert_eq!(ui_scale(None, 480), 1.0);
        assert_eq!(ui_scale(None, 720), 1.0);
        assert_eq!(ui_scale(None, 1080), 1.5);
        assert_eq!(ui_scale(None, 1440), 2.0);
        assert_eq!(ui_scale(None, 1800), 2.5);
        assert_eq!(ui_scale(None, 2160), 3.0);
        assert_eq!(ui_scale(Some(1.0), 2160), 1.0, "fixed: as set");
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
    fn the_display_offers_its_sizes_once_each() {
        let modes = [
            (3840, 2160),
            (1920, 1080),
            (1920, 1080),
            (5120, 2880),
            (320, 200),
            (2560, 1440),
        ];
        assert_eq!(
            sizes(modes.into_iter(), (3840, 2160)),
            [(1920, 1080), (2560, 1440), (3840, 2160)]
        );
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
