//! The launcher (#104): with no game installed, a window over our own
//! picture asks for the player's Anniversary Edition — its `.7z` or its
//! folder, through the system's own file dialog — and installs it
//! (`install::install_from`, on a worker thread) with a progress bar; then
//! the game goes on to its main menu. Drawn with the overlay, its text in
//! an embedded font (Rajdhani, SIL OFL: `assets/launcher/OFL.txt`), as no
//! game art exists yet.

use crate::render::{OverlayDraw, Renderer};
use crate::Gpu;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const BACKGROUND: &[u8] = include_bytes!("../../assets/launcher/background.jpeg");
const FONT: &[u8] = include_bytes!("../../assets/launcher/Rajdhani-SemiBold.ttf");
const BACKGROUND_NAME: &str = "<launcher background>";

/// The panel's size (layout units) and its gap from the window's foot.
const PANEL: (f32, f32) = (560.0, 200.0);
const MARGIN: f32 = 48.0;
const PAD: f32 = 24.0;
const BUTTON: (f32, f32) = (220.0, 42.0);
const TITLE: f32 = 30.0;
const BODY: f32 = 19.0;

const ACCENT: [f32; 4] = [0.35, 0.85, 1.0, 1.0];
const TEXT: [f32; 4] = [0.92, 0.95, 1.0, 1.0];
const DIM: [f32; 4] = [0.70, 0.78, 0.86, 1.0];
const ERROR: [f32; 4] = [1.0, 0.45, 0.45, 1.0];

/// What the player asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    PickArchive,
    PickFolder,
    Cancel,
}

/// An install running: its progress, its cancel, its outcome.
struct Job {
    step: Arc<Mutex<crate::install::Step>>,
    cancel: Arc<AtomicBool>,
    outcome: Arc<Mutex<Option<Result<PathBuf, String>>>>,
}

enum Phase {
    Choose { error: Option<String> },
    Working(Job),
}

pub struct Launcher {
    phase: Phase,
    font: Option<fontdue::Font>,
    /// Text drawn so far: (text, size in pixels) → its texture's name and
    /// size in pixels.
    texts: HashMap<(String, u32), (PathBuf, u32, u32)>,
    background: Option<(u32, u32)>,
    /// The buttons as last drawn (layout units).
    buttons: Vec<([f32; 4], Ask)>,
    hover: Option<Ask>,
}

impl Launcher {
    pub fn new() -> Launcher {
        Launcher {
            phase: Phase::Choose { error: None },
            font: fontdue::Font::from_bytes(FONT, fontdue::FontSettings::default()).ok(),
            texts: HashMap::new(),
            background: None,
            buttons: Vec::new(),
            hover: None,
        }
    }

    /// Starts installing from `path`.
    pub fn start(&mut self, path: PathBuf) {
        let job = Job {
            step: Arc::new(Mutex::new(crate::install::Step {
                what: "Checking the game files".into(),
                done: 0.0,
            })),
            cancel: Arc::new(AtomicBool::new(false)),
            outcome: Arc::new(Mutex::new(None)),
        };
        let (step, cancel, outcome) = (job.step.clone(), job.cancel.clone(), job.outcome.clone());
        std::thread::spawn(move || {
            let r = crate::install::install_from(&path, &mut |s| {
                if let Ok(mut st) = step.lock() {
                    *st = s;
                }
                !cancel.load(Ordering::Relaxed)
            });
            if let Ok(mut o) = outcome.lock() {
                *o = Some(r.map_err(|e| e.to_string()));
            }
        });
        self.phase = Phase::Working(job);
    }

    /// Shows a reason the source wouldn't do (as after a failed install).
    pub fn show_error(&mut self, e: &str) {
        self.phase = Phase::Choose {
            error: Some(e.to_string()),
        };
    }

    /// Shows an install at `done` doing `what`, with no install behind it
    /// (scenario screenshots).
    pub fn show_progress(&mut self, what: &str, done: f32) {
        self.phase = Phase::Working(Job {
            step: Arc::new(Mutex::new(crate::install::Step {
                what: what.to_string(),
                done,
            })),
            cancel: Arc::new(AtomicBool::new(false)),
            outcome: Arc::new(Mutex::new(None)),
        });
    }

    /// The install, once done: the game's root. A failure (or a cancel)
    /// goes back to choosing, its reason shown.
    pub fn poll(&mut self) -> Option<PathBuf> {
        let Phase::Working(job) = &self.phase else {
            return None;
        };
        let outcome = job.outcome.lock().ok()?.take()?;
        match outcome {
            Ok(root) => Some(root),
            Err(e) => {
                let error = (e != oni_convert::source::Problem::Cancelled.to_string()).then_some(e);
                self.phase = Phase::Choose { error };
                None
            }
        }
    }

    /// A click at (`x`, `y`) (layout units).
    pub fn click(&mut self, x: f32, y: f32) -> Option<Ask> {
        let ask = self.at(x, y)?;
        if ask == Ask::Cancel {
            if let Phase::Working(job) = &self.phase {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
        Some(ask)
    }

    /// The pointer moved to (`x`, `y`).
    pub fn hover(&mut self, x: f32, y: f32) {
        self.hover = self.at(x, y);
    }

    fn at(&self, x: f32, y: f32) -> Option<Ask> {
        self.buttons
            .iter()
            .find(|(r, _)| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
            .map(|b| b.1)
    }

    /// The launcher over a `w` × `h` (layout units) window drawn `ui`
    /// pixels a unit.
    pub fn draw(
        &mut self,
        renderer: &mut Renderer,
        gpu: &Gpu,
        w: f32,
        h: f32,
        ui: f32,
    ) -> Vec<OverlayDraw> {
        let mut out = vec![rect([0.0, 0.0, w, h], [0.0, 0.0, 0.0, 1.0])];
        // The picture, covering the window.
        if self.background.is_none() {
            self.background = upload_background(renderer, gpu);
        }
        if let Some((bw, bh)) = self.background {
            let s = (w / bw as f32).max(h / bh as f32);
            let (dw, dh) = (bw as f32 * s, bh as f32 * s);
            let (x, y) = ((w - dw) * 0.5, (h - dh) * 0.5);
            out.push(OverlayDraw {
                texture: Some(PathBuf::from(BACKGROUND_NAME)),
                tris: crate::frame::quad([x, y, x + dw, y + dh]),
                color: [1.0; 4],
                additive: false,
            });
        }
        // The panel.
        let (pw, ph) = (PANEL.0.min(w - 32.0), PANEL.1);
        let (px, py) = ((w - pw) * 0.5, (h - ph - MARGIN).max(16.0));
        out.push(rect([px, py, px + pw, py + ph], [0.02, 0.04, 0.08, 0.82]));
        out.extend(frame(
            [px, py, px + pw, py + ph],
            [ACCENT[0], ACCENT[1], ACCENT[2], 0.55],
        ));
        let (left, right) = (px + PAD, px + pw - PAD);
        let mut y = py + PAD - 4.0;
        self.buttons.clear();
        match &self.phase {
            Phase::Choose { error } => {
                let error = error.clone();
                out.extend(self.text(renderer, gpu, "INSTALL ONI", TITLE, ui, left, y, ACCENT));
                y += TITLE + 8.0;
                let (msg, color) = match &error {
                    Some(e) => (e.clone(), ERROR),
                    None => (
                        "Oni needs the game files from your copy of the Anniversary Edition. Choose its .7z archive or its folder.".to_string(),
                        TEXT,
                    ),
                };
                for line in self.wrap(&msg, BODY, right - left).into_iter().take(3) {
                    out.extend(self.text(renderer, gpu, &line, BODY, ui, left, y, color));
                    y += BODY + 3.0;
                }
                let by = py + ph - PAD - BUTTON.1;
                let gap = (pw - 2.0 * PAD - 2.0 * BUTTON.0).max(8.0);
                for (k, (label, ask)) in [
                    ("Choose archive…", Ask::PickArchive),
                    ("Choose folder…", Ask::PickFolder),
                ]
                .into_iter()
                .enumerate()
                {
                    let bx = left + k as f32 * (BUTTON.0 + gap);
                    out.extend(self.button(renderer, gpu, label, ask, [bx, by], ui));
                }
            }
            Phase::Working(job) => {
                let step = job
                    .step
                    .lock()
                    .map(|s| s.clone())
                    .unwrap_or(crate::install::Step {
                        what: String::new(),
                        done: 0.0,
                    });
                out.extend(self.text(renderer, gpu, "INSTALLING ONI", TITLE, ui, left, y, ACCENT));
                y += TITLE + 12.0;
                out.extend(self.text(renderer, gpu, &step.what, BODY, ui, left, y, TEXT));
                y += BODY + 12.0;
                let done = step.done.clamp(0.0, 1.0);
                let bar = [left, y, right, y + 12.0];
                out.push(rect(bar, [1.0, 1.0, 1.0, 0.12]));
                out.push(rect(
                    [bar[0], bar[1], bar[0] + (bar[2] - bar[0]) * done, bar[3]],
                    ACCENT,
                ));
                let pct = format!("{}%", (done * 100.0) as u32);
                out.extend(self.text(renderer, gpu, &pct, BODY, ui, left, y + 18.0, DIM));
                let by = py + ph - PAD - BUTTON.1;
                out.extend(self.button(
                    renderer,
                    gpu,
                    "Cancel",
                    Ask::Cancel,
                    [right - BUTTON.0, by],
                    ui,
                ));
            }
        }
        out
    }

    fn button(
        &mut self,
        renderer: &mut Renderer,
        gpu: &Gpu,
        label: &str,
        ask: Ask,
        [x, y]: [f32; 2],
        ui: f32,
    ) -> Vec<OverlayDraw> {
        let r = [x, y, x + BUTTON.0, y + BUTTON.1];
        self.buttons.push((r, ask));
        let lit = self.hover == Some(ask);
        let fill = if lit {
            [ACCENT[0], ACCENT[1], ACCENT[2], 0.30]
        } else {
            [ACCENT[0], ACCENT[1], ACCENT[2], 0.12]
        };
        let mut out = vec![rect(r, fill)];
        out.extend(frame(
            r,
            [ACCENT[0], ACCENT[1], ACCENT[2], if lit { 1.0 } else { 0.7 }],
        ));
        let tw = self.measure(label, BODY);
        let (tx, ty) = (x + (BUTTON.0 - tw) * 0.5, y + (BUTTON.1 - BODY) * 0.5 - 1.0);
        out.extend(self.text(renderer, gpu, label, BODY, ui, tx, ty, TEXT));
        out
    }

    /// `s`'s width at `size` (layout units).
    fn measure(&self, s: &str, size: f32) -> f32 {
        let Some(font) = &self.font else { return 0.0 };
        s.chars().map(|c| font.metrics(c, size).advance_width).sum()
    }

    /// `s` in lines no wider than `width`.
    fn wrap(&self, s: &str, size: f32, width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in s.split_whitespace() {
            let next = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if self.measure(&next, size) > width && !line.is_empty() {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = next;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    }

    /// `s` at (`x`, `y`) (its top), `size` units high, in `color`:
    /// rasterized at the window's pixels and kept.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        renderer: &mut Renderer,
        gpu: &Gpu,
        s: &str,
        size: f32,
        ui: f32,
        x: f32,
        y: f32,
        color: [f32; 4],
    ) -> Vec<OverlayDraw> {
        let px = (size * ui).round().max(1.0) as u32;
        let key = (s.to_string(), px);
        if !self.texts.contains_key(&key) {
            let Some(font) = &self.font else {
                return Vec::new();
            };
            let (w, h, rgba) = rasterize(font, s, px as f32);
            let name = PathBuf::from(format!("<launcher text {} {px}>", self.texts.len()));
            renderer.overlay_texture(gpu, &name, w, h, &rgba, false);
            self.texts.insert(key.clone(), (name, w, h));
        }
        let (name, w, h) = self.texts[&key].clone();
        let (x, y) = ((x * ui).round() / ui, (y * ui).round() / ui);
        vec![OverlayDraw {
            texture: Some(name),
            tris: crate::frame::quad([x, y, x + w as f32 / ui, y + h as f32 / ui]),
            color,
            additive: false,
        }]
    }
}

fn rect(r: [f32; 4], color: [f32; 4]) -> OverlayDraw {
    OverlayDraw {
        texture: None,
        tris: crate::frame::quad(r),
        color,
        additive: false,
    }
}

/// A one-unit outline round `r`.
fn frame(r: [f32; 4], color: [f32; 4]) -> Vec<OverlayDraw> {
    let [l, t, rr, b] = r;
    vec![
        rect([l, t, rr, t + 1.0], color),
        rect([l, b - 1.0, rr, b], color),
        rect([l, t, l + 1.0, b], color),
        rect([rr - 1.0, t, rr, b], color),
    ]
}

fn upload_background(renderer: &mut Renderer, gpu: &Gpu) -> Option<(u32, u32)> {
    let mut d = jpeg_decoder::Decoder::new(BACKGROUND);
    let pixels = d.decode().ok()?;
    let info = d.info()?;
    let (w, h) = (u32::from(info.width), u32::from(info.height));
    let rgba: Vec<u8> = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&l| [l, l, l, 255]).collect(),
        _ => return None,
    };
    renderer.overlay_texture(gpu, Path::new(BACKGROUND_NAME), w, h, &rgba, true);
    Some((w, h))
}

/// `s` in white at `px` pixels (its line's height): width, height, RGBA.
fn rasterize(font: &fontdue::Font, s: &str, px: f32) -> (u32, u32, Vec<u8>) {
    let line = font.horizontal_line_metrics(px);
    let (ascent, descent) = line.map_or((px * 0.8, -px * 0.2), |l| (l.ascent, l.descent));
    let h = (ascent - descent).ceil().max(1.0) as usize;
    let glyphs: Vec<(fontdue::Metrics, Vec<u8>)> =
        s.chars().map(|c| font.rasterize(c, px)).collect();
    let w = glyphs
        .iter()
        .map(|(m, _)| m.advance_width)
        .sum::<f32>()
        .ceil()
        .max(1.0) as usize
        + 2;
    let mut rgba = vec![0u8; w * h * 4];
    for px in rgba.chunks_exact_mut(4) {
        px[..3].fill(255);
    }
    let mut pen = 0.0f32;
    for (m, coverage) in &glyphs {
        let gx = (pen + m.xmin as f32).round() as i32;
        let gy = (ascent - m.height as f32 - m.ymin as f32).round() as i32;
        for row in 0..m.height {
            for col in 0..m.width {
                let (x, y) = (gx + col as i32, gy + row as i32);
                if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                    continue;
                }
                let a = &mut rgba[(y as usize * w + x as usize) * 4 + 3];
                *a = (*a).max(coverage[row * m.width + col]);
            }
        }
        pen += m.advance_width;
    }
    (w as u32, h as u32, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_art_and_font_load() {
        let mut d = jpeg_decoder::Decoder::new(BACKGROUND);
        assert!(d.decode().is_ok());
        let font = fontdue::Font::from_bytes(FONT, fontdue::FontSettings::default()).unwrap();
        let (w, h, rgba) = rasterize(&font, "Install Oni", 19.0);
        assert!(w > 50 && h >= 18);
        assert!(rgba.chunks_exact(4).any(|p| p[3] > 200), "ink");
    }

    #[test]
    fn long_messages_wrap_to_the_panel() {
        let l = Launcher::new();
        let lines = l.wrap(
            "Some of the game's files are missing: level4_Final.raw, the original level scripts.",
            BODY,
            300.0,
        );
        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|s| l.measure(s, BODY) <= 300.0));
    }
}
