//! Fonts (`TSFF` families of `TSFT` fonts, their glyphs in `TSGA` arrays)
//! to `common/fonts.json` and one atlas image per font (`common/fonts/`).
//!
//! A glyph's bitmap is 8-bit coverage (stored inverted). The original draws each glyph from a
//! *cell* (`TSiGlyph_DrawToCell`): a box as wide as the glyph and as tall as
//! the font's ascending + descending height, with the bitmap placed at
//! (−origin x, ascending − origin y) and clipped to the box. The atlas holds
//! those cells, so the engine draws a glyph as its cell at the pen (top of
//! the line) and moves the pen on by the glyph's kerning
//! (`TSiContext_DrawTextLine`).

use std::path::Path;

use oni_dat::{Dat, FourCc};
use serde_json::{json, Value};

use crate::{le_i16, le_u16, le_u32};

const TAG_TSFF: FourCc = FourCc(0x5453_4646);
const TAG_TSFT: FourCc = FourCc(0x5453_4654);

/// The atlas's width (cells packed in rows).
const ATLAS_WIDTH: usize = 512;

/// One font: its metrics, and its glyphs' cells packed into an image.
fn font(dat: &Dat<'_>, d: &[u8], file: &str, out: &Path) -> std::io::Result<Option<Value>> {
    // TStFont: pad[6], font_size@6, font_style@8 (u32), ascending@12,
    // descending@14, leading height@16, leading width@18, glyph arrays
    // [256]@20, num_longs@1044, the glyph data (longs[])@1048.
    let (size, style) = (le_u16(d, 6), le_u32(d, 8));
    let (asc, desc) = (le_i16(d, 12).max(0) as usize, le_i16(d, 14).max(0) as usize);
    let cell_h = asc + desc;
    let longs = 1048;
    // The glyphs: (code, kerning, width, origin x, origin y, data offset).
    let mut glyphs = Vec::new();
    for hi in 0..256usize {
        let r = le_u32(d, 20 + hi * 4);
        let Some(arr) = (r & 1 != 0).then(|| dat.instances.get((r >> 8) as usize)).flatten().and_then(|i| dat.instance_data(i)) else {
            continue;
        };
        for lo in 0..256usize {
            let g = lo * 20;
            if g + 20 > arr.len() {
                break;
            }
            let (code, kerning, width, height) = (le_u16(arr, g), le_u16(arr, g + 2), le_i16(arr, g + 4), le_i16(arr, g + 6));
            // (An unused slot: no code, nothing to draw, no advance.)
            if code == 0 && kerning == 0 && width == 0 {
                continue;
            }
            glyphs.push((code, kerning, width.max(0) as usize, height.max(0) as usize, le_i16(arr, g + 8), le_i16(arr, g + 10), le_u32(arr, g + 12) as usize));
        }
    }
    if glyphs.is_empty() {
        return Ok(None);
    }
    // Pack the cells in rows.
    let (mut x, mut y) = (0usize, 0usize);
    let mut places = Vec::with_capacity(glyphs.len());
    for g in &glyphs {
        if x + g.2 > ATLAS_WIDTH {
            x = 0;
            y += cell_h + 1;
        }
        places.push((x, y));
        x += g.2 + 1;
    }
    let atlas_h = (y + cell_h + 1).next_power_of_two();
    let mut rgba = vec![0u8; ATLAS_WIDTH * atlas_h * 4];
    // White, with the coverage as alpha.
    for p in rgba.as_chunks_mut::<4>().0 {
        p[..3].copy_from_slice(&[255, 255, 255]);
    }
    let mut table = serde_json::Map::new();
    for (g, &(cx, cy)) in glyphs.iter().zip(&places) {
        let &(code, kerning, w, h, ox, oy, offset) = g;
        let src = longs + offset * 4;
        for gy in 0..h {
            for gx in 0..w {
                let (tx, ty) = (gx as i32 - i32::from(ox), gy as i32 + asc as i32 - i32::from(oy));
                if tx < 0 || ty < 0 || tx as usize >= w || ty as usize >= cell_h {
                    continue;
                }
                // (Stored inverted: 255 is empty — the original's copy
                // into an alpha texture is `0xFF − texel`.)
                let a = 255 - d.get(src + gy * w + gx).copied().unwrap_or(255);
                rgba[((cy + ty as usize) * ATLAS_WIDTH + cx + tx as usize) * 4 + 3] = a;
            }
        }
        table.insert(code.to_string(), json!([cx, cy, w, kerning]));
    }
    std::fs::create_dir_all(out.join("fonts"))?;
    crate::write_png(&out.join("fonts").join(format!("{file}.png")), ATLAS_WIDTH as u32, atlas_h as u32, &rgba)?;
    let style = ["plain", "bold", "italic"].get(style as usize).copied().unwrap_or("plain");
    Ok(Some(json!({
        "size": size,
        "style": style,
        "ascendingHeight": asc,
        "descendingHeight": desc,
        "leadingHeight": le_i16(d, 16),
        "leadingWidth": le_i16(d, 18),
        "atlas": format!("fonts/{file}.png"),
        "glyphs": table,
    })))
}

/// Writes `fonts.json` (each family's fonts: metrics and glyphs, by code:
/// `[x, y, width, kerning]` of its cell in the atlas) and the atlases.
/// Returns how many fonts.
pub fn convert(dat: &Dat<'_>, out: &Path) -> std::io::Result<usize> {
    let mut families = serde_json::Map::new();
    let mut count = 0;
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_TSFF) {
        let (Some(d), Some(name)) = (dat.instance_data(inst), inst.name.as_deref()) else { continue };
        // TStFontFamily: pad[16], font language@16, num_fonts@20, fonts@24.
        let n = le_u32(d, 20) as usize;
        let mut fonts = Vec::new();
        for k in 0..n {
            let r = le_u32(d, 24 + k * 4);
            let Some(f) = (r & 1 != 0).then(|| dat.instances.get((r >> 8) as usize)).flatten().filter(|f| f.tag == TAG_TSFT) else {
                continue;
            };
            let Some(fd) = dat.instance_data(f) else { continue };
            let file = format!("{}_{}_{}", crate::sanitize(name), le_u16(fd, 6), le_u32(fd, 8));
            if let Some(v) = font(dat, fd, &file, out)? {
                fonts.push(v);
                count += 1;
            }
        }
        // The characters a line may break after (`breaking_2` of its
        // `TSFL` language: five 64-byte strings).
        let lang = le_u32(d, 16);
        let breaking = (lang & 1 != 0)
            .then(|| dat.instances.get((lang >> 8) as usize))
            .flatten()
            .and_then(|l| dat.instance_data(l))
            .map(|l| crate::cstr(l, 64, 64))
            .unwrap_or_default();
        families.insert(name.to_string(), json!({ "fonts": fonts, "breaking": breaking }));
    }
    if families.is_empty() {
        return Ok(0);
    }
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join("fonts.json"), serde_json::to_string_pretty(&json!({ "families": families }))?)?;
    Ok(count)
}
