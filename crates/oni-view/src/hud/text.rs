//! Text on screen (`BFW_TextSystem.c`, `BFW_Console.c`):
//! the fonts (`common/fonts.json`), laying a string
//! out in lines of coloured segments (`TSrContext_FormatString`: `[c.…]`
//! colour spans, wrapping after breaking characters) and the text areas the
//! game prints messages and subtitles into (`COtTextArea`: entries that fade
//! out, stacked from the bottom).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One font: its metrics, and its glyphs' cells in its atlas.
#[derive(Clone, Debug)]
pub struct Font {
    pub size: u16,
    pub style: String,
    pub ascending: i32,
    pub descending: i32,
    pub leading: i32,
    pub atlas: PathBuf,
    pub atlas_size: [f32; 2],
    /// By character code: the cell's place in the atlas, its width and the
    /// pen's advance.
    pub glyphs: HashMap<u16, [i32; 4]>,
}

impl Font {
    /// A line's height (`TSrFont_GetLineHeight`).
    pub fn line_height(&self) -> i32 {
        self.ascending + self.descending + self.leading
    }

    /// A cell's height.
    pub fn cell_height(&self) -> i32 {
        self.ascending + self.descending
    }

    pub fn glyph(&self, c: char) -> Option<[i32; 4]> {
        // (The font's codes are single bytes; the game's text is ASCII.)
        u16::try_from(u32::from(c))
            .ok()
            .and_then(|code| self.glyphs.get(&code).copied())
    }
}

/// A font family's fonts.
#[derive(Clone, Debug, Default)]
pub struct Fonts {
    pub fonts: Vec<Font>,
    /// What a line may break after.
    pub breaking: Vec<char>,
}

impl Fonts {
    /// Loads the default family (Tahoma) from `common/fonts.json`.
    pub fn load(common: &Path) -> Fonts {
        let Some(doc) =
            crate::json::read_some::<oni_schema::files::FontsFile>(common.join("fonts.json"))
        else {
            return Fonts::default();
        };
        let Some(fam) = doc
            .families
            .get("Tahoma")
            .or_else(|| doc.families.values().next())
        else {
            return Fonts {
                fonts: Vec::new(),
                breaking: vec![' '],
            };
        };
        let fonts = fam
            .fonts
            .iter()
            .map(|f| {
                let atlas = common.join(&f.atlas);
                let size = image_size(&atlas).unwrap_or([512.0, 512.0]);
                Font {
                    size: f.size,
                    style: f.style.clone(),
                    ascending: f.ascending_height as i32,
                    descending: f.descending_height as i32,
                    leading: i32::from(f.leading_height),
                    atlas,
                    atlas_size: size,
                    glyphs: f
                        .glyphs
                        .iter()
                        .filter_map(|(k, g)| Some((k.parse().ok()?, g.map(|v| v as i32))))
                        .collect(),
                }
            })
            .collect();
        Fonts {
            fonts,
            breaking: fam.breaking.chars().collect(),
        }
    }

    /// The font for a size in a style (`TSiContext_GetBestFont`): as
    /// [`Fonts::best`], in that style where the size has it.
    pub fn best_styled(&self, size: u16, style: &str) -> Option<&Font> {
        let plain = self.best(size)?;
        self.fonts
            .iter()
            .find(|f| f.size == plain.size && f.style == style)
            .or(Some(plain))
    }

    /// The font for a size (`TSiContext_GetBestFont`): that size, else the
    /// nearest smaller one, else the nearest larger; plain if the style
    /// isn't there.
    pub fn best(&self, size: u16) -> Option<&Font> {
        let at = |s: u16| {
            self.fonts
                .iter()
                .find(|f| f.size == s && f.style == "plain")
        };
        (1..=size)
            .rev()
            .find_map(at)
            .or_else(|| (size..=48).find_map(at))
    }
}

/// A PNG's width and height.
fn image_size(path: &Path) -> Option<[f32; 2]> {
    let d = std::fs::File::open(path).ok()?;
    let r = png::Decoder::new(std::io::BufReader::new(d))
        .read_info()
        .ok()?;
    let i = r.info();
    Some([i.width as f32, i.height as f32])
}

/// `[x.` … `]` colours (`TSgColorFormattingCharacters`), as RGB.
fn span_color(c: char) -> Option<u32> {
    Some(match c {
        'r' => 0xEB5050,
        'y' => 0xFCFF1C,
        'b' => 0x93BBE9,
        'u' => 0xF67603,
        'g' => 0xA2DAA5,
        'l' => 0xBE90D9,
        'o' => 0xFBA500,
        'c' => 0x93EAEB,
        _ => return None,
    })
}

/// How a line sits across its bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
}

/// A run of text in one colour: where its pen starts (x, the line's top),
/// how wide it is, its colour (RGB).
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub text: String,
    pub x: i32,
    pub top: i32,
    pub width: i32,
    pub color: u32,
}

/// A rectangle (left, top, right, bottom).
pub type Rect = [i32; 4];

/// Lays `text` out (`TSrContext_FormatString`): segments split at colour
/// spans (with `formatting`), lines wrapped where the next glyph would pass
/// the bounds' width — back to just after the last breaking character, else
/// at the segment's start when the line has earlier segments — and lines
/// placed from `top` down, a line height apart, aligned in the bounds.
#[allow(clippy::too_many_arguments)]
pub fn format(
    font: &Font,
    breaking: &[char],
    text: &str,
    bounds: Rect,
    left: i32,
    top: i32,
    align: Align,
    formatting: bool,
    color: u32,
) -> Vec<Segment> {
    let chars: Vec<char> = text.chars().collect();
    let width = bounds[2] - bounds[0];
    let mut out: Vec<Segment> = Vec::new();
    let mut lines: Vec<(usize, i32)> = Vec::new(); // (first segment, width)
    let (mut i, mut shade, mut begin_formatting) = (0usize, color, false);
    let (mut line_start, mut line_width, mut new_line) = (0usize, 0i32, true);
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 10_000 {
            break;
        }
        if new_line {
            line_start = out.len();
            line_width = 0;
        }
        let mut start = i;
        let (mut n, mut seg_w, mut prev_break) = (0usize, 0i32, 0usize);
        let (mut done, mut intraline, mut restore) = (false, false, false);
        loop {
            let Some(&c) = chars.get(i) else {
                done = true;
                break;
            };
            if formatting {
                if c == '[' {
                    i += 1;
                    begin_formatting = true;
                    intraline = true;
                    break;
                } else if c == ']' {
                    i += 1;
                    begin_formatting = false;
                    restore = true;
                    intraline = true;
                    break;
                } else if begin_formatting {
                    if c == '.' {
                        i += 1;
                        start = i;
                        begin_formatting = false;
                        continue;
                    } else if let Some(s) = span_color(c) {
                        shade = s;
                        i += 1;
                        continue;
                    } else {
                        start = i;
                        begin_formatting = false;
                    }
                }
            }
            n += 1;
            let mut must_break = false;
            if let Some(g) = font.glyph(c) {
                seg_w += g[3];
                must_break = line_width + seg_w > width;
            }
            i += 1;
            if must_break {
                begin_formatting = false;
                break;
            } else if breaking.contains(&c) {
                prev_break = n;
            }
        }
        if !done && !intraline {
            // Wrap: back to after the last break, else (with earlier
            // segments on the line) before this one.
            if prev_break > 0 {
                for &c in &chars[start + prev_break..start + n] {
                    if let Some(g) = font.glyph(c) {
                        seg_w -= g[3];
                    }
                }
                n = prev_break;
                i = start + n;
            } else if out.len() > line_start {
                i = start;
                n = 0;
            }
        }
        if n > 0 {
            out.push(Segment {
                text: chars[start..start + n].iter().collect(),
                x: 0,
                top: 0,
                width: seg_w,
                color: shade,
            });
            line_width += seg_w;
        }
        if !intraline {
            lines.push((line_start, line_width));
            new_line = true;
        } else {
            new_line = false;
        }
        if restore {
            shade = color;
        }
        if done || out.len() + 1 >= 24 {
            break;
        }
    }
    // Place the lines.
    let mut y = top;
    for (k, &(first, w)) in lines.iter().enumerate() {
        let end = lines.get(k + 1).map_or(out.len(), |l| l.0);
        let mut x = match align {
            Align::Center => bounds[0] + ((width - w) >> 1),
            Align::Left => left,
        };
        for s in &mut out[first..end] {
            s.x = x;
            s.top = y;
            x += s.width;
        }
        y += font.line_height();
    }
    out
}

/// One printed line (`COtTextEntry`): its text, the ticks it has left (0:
/// for good) and what it was printed as.
#[derive(Clone, Debug)]
pub struct Entry {
    pub text: String,
    pub timer: u32,
    pub tag: Option<String>,
}

/// A text area (`COtTextArea`): where, how it lays text out, how many
/// entries it holds, and whether one that overflows fades at once.
#[derive(Clone, Debug)]
pub struct TextArea {
    pub entries: Vec<Entry>,
    pub max: usize,
    pub align: Align,
    pub formatting: bool,
    /// Its font size at 480 lines high (scaled with the screen).
    pub font_size: u16,
    pub fade_over_bounds: bool,
}

/// A line told to go fades over this many ticks (`COcImmediateFadeTime`).
pub const IMMEDIATE_FADE: u32 = 64;

impl TextArea {
    pub fn new(
        max: usize,
        align: Align,
        formatting: bool,
        font_size: u16,
        fade_over_bounds: bool,
    ) -> TextArea {
        TextArea {
            entries: Vec::new(),
            max,
            align,
            formatting,
            font_size,
            fade_over_bounds,
        }
    }

    /// Prints a line (`COrTextArea_Print`): full, the oldest goes.
    pub fn print(&mut self, text: &str, tag: Option<&str>, timer: u32) {
        if text.is_empty() || self.max == 0 {
            return;
        }
        if self.entries.len() >= self.max {
            self.entries.remove(0);
        }
        self.entries.push(Entry {
            text: text.to_string(),
            timer,
            tag: tag.map(str::to_string),
        });
    }

    /// Whether a line printed as `tag` is up (`COrTextArea_IdentifierVisible`).
    pub fn visible(&self, tag: &str) -> bool {
        self.entries.iter().any(|e| e.tag.as_deref() == Some(tag))
    }

    /// Lines printed as `tag` (all, without one) fade out now
    /// (`COrTextArea_FadeByIdentifier`).
    pub fn fade(&mut self, tag: Option<&str>) -> bool {
        let mut found = false;
        for e in &mut self.entries {
            if tag.is_some_and(|t| e.tag.as_deref() != Some(t)) {
                continue;
            }
            if e.timer == 0 || e.timer > IMMEDIATE_FADE {
                e.timer = IMMEDIATE_FADE;
            }
            found = true;
        }
        found
    }

    /// One tick (`COrTextArea_Update`): timed lines run down and go.
    pub fn tick(&mut self) {
        self.entries.retain_mut(|e| {
            if e.timer == 0 {
                return true;
            }
            e.timer -= 1;
            e.timer > 0
        });
    }

    /// Lays the area out (`COrTextArea_Display`): newest at the bottom,
    /// each a little apart; a line fading out (its last 64 ticks) at 4 × its
    /// ticks left of opacity. One that overflows the top fades at once, or
    /// (not fading over bounds) stops the drawing.
    pub fn layout(
        &mut self,
        fonts: &Fonts,
        bounds: Rect,
        screen_h: f32,
    ) -> Vec<(Segment, f32, String)> {
        let size = ((u32::from(self.font_size) * screen_h as u32) / 480) as u16;
        let Some(font) = fonts.best(size.max(1)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut y = bounds[3];
        for e in self.entries.iter_mut().rev() {
            let alpha = if e.timer == 0 || e.timer * 4 > 255 {
                1.0
            } else {
                (e.timer * 4) as f32 / 255.0
            };
            // (Laid out from the bounds' top as its first line's baseline,
            // so the text sits its ascending height above its slot.)
            let segs = format(
                font,
                &fonts.breaking,
                &e.text,
                bounds,
                bounds[0],
                bounds[1] - font.ascending,
                self.align,
                self.formatting,
                0xFFFFFF,
            );
            let (Some(first), Some(last)) = (segs.first(), segs.last()) else {
                continue;
            };
            let height = last.top + font.line_height() - first.top + 2;
            y -= height;
            if y < bounds[1] {
                if self.fade_over_bounds {
                    if e.timer == 0 || e.timer > IMMEDIATE_FADE {
                        e.timer = IMMEDIATE_FADE;
                    }
                } else {
                    break;
                }
            }
            let dy = y - bounds[1];
            for mut s in segs {
                s.top += dy;
                out.push((s, alpha, font.atlas.display().to_string()));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font whose every printable glyph is 5 wide.
    fn font() -> Font {
        Font {
            size: 7,
            style: "plain".into(),
            ascending: 9,
            descending: 2,
            leading: 2,
            atlas: PathBuf::new(),
            atlas_size: [512.0, 512.0],
            glyphs: (32u16..127).map(|c| (c, [0, 0, 5, 5])).collect(),
        }
    }

    #[test]
    fn colour_spans_split_segments() {
        let s = format(
            &font(),
            &[' '],
            "Press [c.Q] now",
            [0, 0, 500, 100],
            0,
            0,
            Align::Left,
            true,
            0xFFFFFF,
        );
        let texts: Vec<_> = s.iter().map(|s| (s.text.as_str(), s.color)).collect();
        assert_eq!(
            texts,
            vec![("Press ", 0xFFFFFF), ("Q", 0x93EAEB), (" now", 0xFFFFFF)]
        );
        assert_eq!(s[1].x, 30);
        assert_eq!(s[2].x, 35);
    }

    #[test]
    fn long_lines_wrap_after_a_space_and_centre() {
        // 10 glyphs fit in 50.
        let s = format(
            &font(),
            &[' '],
            "aaaa bbbb cccc",
            [0, 0, 50, 100],
            0,
            0,
            Align::Center,
            true,
            0xFFFFFF,
        );
        let lines: Vec<_> = s.iter().map(|s| (s.text.as_str(), s.top)).collect();
        assert_eq!(lines, vec![("aaaa bbbb ", 0), ("cccc", 13)]);
        assert_eq!(s[1].x, (50 - 20) / 2);
    }

    #[test]
    fn lines_fade_out_and_go() {
        let mut a = TextArea::new(3, Align::Center, true, 12, false);
        a.print("one", Some("x"), 0);
        a.print("two", None, 100);
        assert!(a.visible("x"));
        a.fade(Some("x"));
        for _ in 0..IMMEDIATE_FADE {
            a.tick();
        }
        assert!(!a.visible("x"), "faded out");
        assert_eq!(a.entries.len(), 1, "the timed one still up");
        for _ in IMMEDIATE_FADE..100 {
            a.tick();
        }
        assert_eq!(a.entries.len(), 0, "and then it ran out too");
    }
}
