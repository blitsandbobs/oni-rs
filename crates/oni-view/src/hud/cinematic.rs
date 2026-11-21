//! Cinematic inserts (`docs/HUD.md`, *Cutscene portraits*; `Oni_Cinematics.c`):
//! the framed pictures a cutscene slides onto the screen — a speaker's face,
//! a name tag. `cinematic_start` puts one at one of 25 positions on a
//! 1024 × 728 virtual screen (the bottom 40 lines kept clear for subtitles),
//! sliding it from a start position to an end one; `cinematic_stop` slides
//! it to another and takes it away; `end_cutscene` takes them all. Each is
//! drawn unlit, unfiltered, in the `cinematic_border` frame.

use std::path::PathBuf;

const SCREEN_W: f32 = 1024.0;
const SCREEN_H: f32 = 768.0;
/// The area they use (`OCcCinematic_Width` / `_Height`).
const AREA_W: f32 = SCREEN_W;
const AREA_H: f32 = SCREEN_H - 40.0;
/// The insets from the screen's edges (`ONgXOffset` / `ONgYOffset`).
const X_OFFSET: f32 = 20.0;
const Y_OFFSET: f32 = 65.0;
/// The highest position index.
const POSITIONS: u8 = 25;

/// How a position's coordinate is offset: not, by +offset, by −offset.
#[derive(Clone, Copy)]
enum Mod {
    No,
    Add,
    Sub,
}

/// Where a picture sits against a position (`OCtPart`): row (top / centre
/// / bottom) and column (left / centre / right).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Start,
    Center,
    End,
}

/// The 26 positions (`ONgIndexPositions`): x and y offsets, whether the
/// area's width / height is added, and the picture's row and column side.
const POSITION: [(Mod, Mod, bool, bool, Side, Side); 26] = {
    use Mod::*;
    use Side::*;
    [
        (No, No, false, false, Start, Start), // 0 (none: the top left)
        (Add, Add, false, false, Start, Start),
        (No, Add, false, false, Start, Center),
        (Sub, Add, true, false, Start, End),
        (Add, No, false, false, Center, Start),
        (No, No, false, false, Center, Center),
        (Sub, No, true, false, Center, End),
        (Add, Sub, false, true, End, Start),
        (No, Sub, false, true, End, Center),
        (Sub, Sub, true, true, End, End),
        (Sub, Sub, false, false, End, End), // 10: off the screen…
        (Add, Sub, false, false, End, Start),
        (No, Sub, false, false, End, Center),
        (Sub, Sub, true, false, End, End),
        (Add, Sub, true, false, End, Start),
        (Sub, Add, false, false, Start, End),
        (Add, Add, true, false, Start, Start),
        (Sub, No, false, false, Center, End),
        (Add, No, true, false, Center, Start),
        (Sub, Sub, false, true, End, End),
        (Add, Sub, true, true, End, Start),
        (Sub, Add, false, true, Start, End),
        (Add, Add, false, true, Start, Start),
        (No, Add, false, true, Start, Center),
        (Sub, Add, true, true, Start, End),
        (Add, Add, true, true, Start, Start),
    ]
};

/// One insert.
#[derive(Clone, Debug)]
pub struct Cinematic {
    /// Its texture's name, and file.
    pub name: String,
    pub texture: PathBuf,
    pub mirror: bool,
    /// Its size and top left on the virtual screen.
    pub size: [f32; 2],
    pub at: [f32; 2],
    delta: [f32; 2],
    steps: u32,
    end: u8,
    /// Arrived (placed exactly), and going away once there.
    at_end: bool,
    stopping: bool,
}

/// Where position `index` puts a picture of `size` (its top left;
/// `OCiCinematic_CalcPositionFromIndex`).
fn position(index: u8, size: [f32; 2]) -> [f32; 2] {
    let (mx, my, add_w, add_h, row, column) = POSITION[usize::from(index)];
    let offset = |m: Mod, d: f32| match m {
        Mod::No => 0.0,
        Mod::Add => d,
        Mod::Sub => -d,
    };
    let mut x = offset(mx, X_OFFSET) + if add_w { AREA_W } else { 0.0 };
    let mut y = offset(my, Y_OFFSET) + if add_h { AREA_H } else { 0.0 };
    match row {
        Side::Start => {}
        Side::Center => y = (AREA_H - size[1]) / 2.0,
        Side::End => y -= size[1],
    }
    match column {
        Side::Start => {}
        Side::Center => x = (AREA_W - size[0]) / 2.0,
        Side::End => x -= size[0],
    }
    [x, y]
}

/// The move from `from` to position `to` at `speed` a tick: the step, and
/// how many steps (none when they're the same position).
fn motion(from: [f32; 2], to: [f32; 2], speed: f32) -> ([f32; 2], u32) {
    let d = [to[0] - from[0], to[1] - from[1]];
    let length = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if length <= 0.0 || speed <= 0.0 {
        return ([0.0, 0.0], 0);
    }
    (
        [d[0] / length * speed, d[1] / length * speed],
        (length / speed) as u32,
    )
}

/// The inserts showing.
#[derive(Clone, Debug, Default)]
pub struct Cinematics {
    pub list: Vec<Cinematic>,
}

impl Cinematics {
    /// `cinematic_start name width height start end speed mirror`
    /// (`OCrCinematic_Start`): `texture` (None: not found — nothing, as in
    /// the original) of `natural` size, drawn `width` × `height` on a 640 ×
    /// 480 scale (−1: its own size), from position `start` to `end` at
    /// `speed` units a tick.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        name: &str,
        texture: Option<(PathBuf, [f32; 2])>,
        width: i32,
        height: i32,
        start: i32,
        end: i32,
        speed: f32,
        mirror: bool,
    ) {
        let Some((texture, natural)) = texture else {
            return;
        };
        let (Ok(start), Ok(end)) = (u8::try_from(start), u8::try_from(end)) else {
            return;
        };
        if start > POSITIONS
            || end > POSITIONS
            || width < -1
            || height < -1
            || (start != end && speed <= 0.0)
        {
            return;
        }
        let w = if width == -1 {
            natural[0]
        } else {
            width as f32
        };
        let h = if height == -1 {
            natural[1]
        } else {
            height as f32
        };
        // (Meant to scale the 640 × 480 sizes onto the 1024 × 768 screen —
        // `× (1024 / 640)` — but the original's integer division makes both
        // factors 1: the sizes are
        // taken as they are, and the picture comes out smaller than meant —
        // kept, as shipped.)
        let size = [w.trunc(), h.trunc()];
        let at = position(start, size);
        let (delta, steps) = if start == end {
            ([0.0, 0.0], 0)
        } else {
            motion(at, position(end, size), speed)
        };
        self.list.push(Cinematic {
            name: name.to_string(),
            texture,
            mirror,
            size,
            at,
            delta,
            steps,
            end,
            at_end: false,
            stopping: false,
        });
    }

    /// `cinematic_stop name end speed` (`OCrCinematic_Stop`): the first
    /// insert of that texture slides from where it was headed to position
    /// `end`, and goes.
    pub fn stop(&mut self, name: &str, end: i32, speed: f32) {
        let Some(c) = self
            .list
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(name))
        else {
            return;
        };
        let Ok(end) = u8::try_from(end) else { return };
        if end > POSITIONS {
            return;
        }
        let start = c.end;
        c.at_end = false;
        c.stopping = true;
        c.end = end;
        c.at = position(start, c.size);
        (c.delta, c.steps) = if end == start {
            ([0.0, 0.0], 0)
        } else {
            motion(c.at, position(end, c.size), speed)
        };
    }

    /// `end_cutscene` (`OCrCinematic_DeleteAll`).
    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// One tick (`OCrCinematic_Update`): each moves a step; arrived, it is
    /// placed exactly — and one stopping goes.
    pub fn tick(&mut self) {
        for c in &mut self.list {
            if c.steps > 0 {
                c.steps -= 1;
                c.at[0] += c.delta[0];
                c.at[1] += c.delta[1];
            } else if !c.at_end {
                c.at = position(c.end, c.size);
                c.at_end = true;
            }
        }
        self.list.retain(|c| !(c.at_end && c.stopping));
    }

    /// Each insert's rectangle on a `w` × `h` screen (left, top, right,
    /// bottom; rounded as `OCiCinematic_Draw` rounds them).
    pub fn rects(&self, w: f32, h: f32) -> Vec<(&Cinematic, [f32; 4])> {
        let (sx, sy) = (w / SCREEN_W, h / SCREEN_H);
        self.list
            .iter()
            .map(|c| {
                (
                    c,
                    [
                        (c.at[0] * sx).round(),
                        (c.at[1] * sy).round(),
                        ((c.at[0] + c.size[0]) * sx).round(),
                        ((c.at[1] + c.size[1]) * sy).round(),
                    ],
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_inset_and_aligned() {
        let size = [288.0, 288.0];
        // Top left, inset.
        assert_eq!(position(1, size), [20.0, 65.0]);
        // Top right: its right edge 20 in from the area's.
        assert_eq!(position(3, size), [1024.0 - 20.0 - 288.0, 65.0]);
        // Bottom centre: centred, its bottom 65 above the area's.
        assert_eq!(
            position(8, size),
            [(1024.0 - 288.0) / 2.0, 728.0 - 65.0 - 288.0]
        );
        // 16: off the right edge (the area's width added, then left-aligned).
        assert_eq!(position(16, size), [1024.0 + 20.0, 65.0]);
    }

    #[test]
    fn it_slides_in_then_stops_and_goes() {
        let mut c = Cinematics::default();
        // 180 wide (as the integer factor of 1 leaves it); from off the right
        // (16) to the top right (3) at 20 a tick.
        c.start(
            "tag",
            Some((PathBuf::from("tag.png"), [128.0, 128.0])),
            180,
            180,
            16,
            3,
            20.0,
            false,
        );
        assert_eq!(c.list[0].size, [180.0, 180.0]);
        assert_eq!(c.list[0].at, position(16, [180.0, 180.0]));
        for _ in 0..40 {
            c.tick();
        }
        assert_eq!(c.list[0].at, position(3, [180.0, 180.0]), "arrived exactly");
        c.stop("TAG", 16, 30.0);
        for _ in 0..40 {
            c.tick();
        }
        assert!(c.list.is_empty(), "gone once slid away");
        // A missing texture shows nothing.
        c.start("none", None, 180, 180, 1, 1, 0.0, false);
        assert!(c.list.is_empty());
    }
}
