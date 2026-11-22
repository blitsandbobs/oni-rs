//! The HUD help overlay (`ONiInGameUI_DisplayHelp`; `docs/HUD.md`, *Help
//! overlay*): on the pause screen's Help page (or by `ui_show_help`), a
//! texture over each ring with lines out to green labels naming its parts
//! (`common/hud_help.json`, from level 0's `hud_help_info`).

use std::path::Path;

/// One side's overlay: its texture, and where its middle sits from the
/// ring's centre.
#[derive(Clone, Debug, Default)]
pub struct Side {
    pub texture: Option<String>,
    pub offset: [i32; 2],
}

/// A label: its text, where it is pinned from its side's overlay's top-left
/// corner, and which side it labels.
#[derive(Clone, Debug)]
pub struct Label {
    pub text: String,
    pub offset: [i32; 2],
    pub right: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Help {
    pub left: Side,
    pub right: Side,
    pub labels: Vec<Label>,
}

/// A label's horizontal nudge off its overlay (`cHelpTextXOffset`): right
/// of the left one's corner, left of the right one's.
pub const TEXT_X_OFFSET: i32 = 3;
/// The labels' size (`TScFontSize_Default`) and shade (`IMcShade_Green`).
pub const TEXT_SIZE: u16 = 7;
pub const TEXT_COLOR: u32 = 0x00FF00;

impl Help {
    pub fn load(common: &Path) -> Option<Help> {
        let doc: serde_json::Value = crate::json::read(common.join("hud_help.json"))?;
        let pair = |v: &serde_json::Value| {
            [
                v[0].as_i64().unwrap_or(0) as i32,
                v[1].as_i64().unwrap_or(0) as i32,
            ]
        };
        let side = |v: &serde_json::Value| Side {
            texture: v["texture"].as_str().map(str::to_string),
            offset: pair(&v["offset"]),
        };
        Some(Help {
            left: side(&doc["left"]),
            right: side(&doc["right"]),
            labels: doc["items"]
                .as_array()?
                .iter()
                .map(|i| Label {
                    text: i["text"].as_str().unwrap_or("").to_string(),
                    offset: pair(&i["offset"]),
                    right: i["right"].as_bool().unwrap_or(false),
                })
                .collect(),
        })
    }
}

/// Where a side's overlay goes (its top-left): the ring's centre (rounded)
/// plus the side's offset, less half the overlay's size (half the ring
/// texture's when the side has no overlay).
pub fn corner(center: [f32; 2], offset: [i32; 2], size: [f32; 2]) -> [i32; 2] {
    [
        center[0].round() as i32 + offset[0] - size[0] as i32 / 2,
        center[1].round() as i32 + offset[1] - size[1] as i32 / 2,
    ]
}

/// A label's line (`TSrContext_FormatString` then the overlay's shift): its
/// pen's start and its line's top. The left side's labels start at their
/// point, the right side's end there; each is centred on it vertically.
pub fn place(label: &Label, corner: [i32; 2], width: i32, line_height: i32) -> [i32; 2] {
    let (x, y) = if label.right {
        (corner[0] - TEXT_X_OFFSET, corner[1])
    } else {
        (corner[0] + TEXT_X_OFFSET, corner[1])
    };
    let (x, y) = (x + label.offset[0], y + label.offset[1]);
    let x = if label.right { x - width } else { x };
    [x, y - line_height / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_pin_left_and_right_and_centre_on_their_point() {
        let l = |right| Label {
            text: "X".into(),
            offset: [10, 20],
            right,
        };
        // A 64 × 64 overlay offset 55 right of a ring centred at (100, 400).
        let c = corner([100.4, 400.0], [55, 0], [64.0, 64.0]);
        assert_eq!(c, [123, 368]);
        assert_eq!(place(&l(false), c, 30, 13), [136, 382]);
        assert_eq!(place(&l(true), c, 30, 13), [100, 382]);
    }
}
