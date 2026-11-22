//! The mouse in the window (`docs/MENU.md`, *The mouse*): grabbed while a
//! level plays (it turns and aims), free with the original's arrow over the
//! menus, the pause screen and a console's text, and never grabbed by a
//! window without the focus.

use std::path::{Path, PathBuf};

use winit::dpi::PhysicalPosition;
use winit::window::CursorGrabMode;

use super::State;

/// The arrow's size and the point of it on the mouse (`WMcCursor_Width`,
/// `_Height`, `_Center_X`, `_Center_Y`).
const ARROW: f32 = 32.0;
const ARROW_CENTER: f32 = 16.0;

/// The mouse as last set.
#[derive(Default)]
pub(super) struct Mouse {
    /// The window has the focus (the mouse is grabbed only then).
    pub focused: bool,
    /// The pointer is over the window.
    pub inside: bool,
    /// Grabbed: locked, hidden, turning the player.
    pub grabbed: bool,
    hidden: bool,
    /// The arrow drawn at the pointer.
    arrow: bool,
    /// The arrow's texture (`WMrCursor_Get(WMcCursorType_Arrow)`: level 0's
    /// `cursor_list` gives `ps_cursor_arrow`), once looked up.
    texture: Option<Option<PathBuf>>,
}

impl Mouse {
    pub fn new(focused: bool) -> Mouse {
        Mouse {
            focused,
            ..Default::default()
        }
    }
}

/// The arrow's texture: its part specification's (`common/part_specs.json`).
fn arrow_texture(common: &Path) -> Option<PathBuf> {
    let doc: serde_json::Value = crate::json::read(common.join("part_specs.json"))?;
    let name = doc["partSpecs"]["ps_cursor_arrow"]["texture"].as_str()?;
    Some(common.join("textures").join(format!("{name}.png"))).filter(|p| p.exists())
}

impl State {
    /// A screen of the game's own up: the menu, the pause screen, a
    /// console's text (the original's window manager active).
    fn ui_up(&self) -> bool {
        self.menu_open() || self.play.as_ref().is_some_and(|p| p.world.hud.modal())
    }

    /// The mouse made to match what is up (each frame): grabbed only while
    /// a level plays with no screen of the game's own up, the player wanting
    /// it (`captured`: Esc lets go in the developers' launches) and the
    /// window focused — the pointer brought to the window's middle before it
    /// is locked, so a click never lands outside. Over the game's screens
    /// the pointer is the original's arrow (`WMrCursor_Draw`).
    pub(super) fn sync_mouse(&mut self) {
        let ui = self.ui_up();
        let grab = self.mouse.focused && self.captured && self.play.is_some() && !ui;
        if grab != self.mouse.grabbed {
            self.mouse.grabbed = grab;
            self.mouse_held.clear();
            if grab {
                let size = self.window.inner_size();
                let _ = self
                    .window
                    .set_cursor_position(PhysicalPosition::new(size.width / 2, size.height / 2));
                let scale = self.window.scale_factor() as f32;
                self.cursor = [
                    size.width as f32 / 2.0 / scale,
                    size.height as f32 / 2.0 / scale,
                ];
                let _ = self
                    .window
                    .set_cursor_grab(CursorGrabMode::Locked)
                    .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));
            } else {
                let _ = self.window.set_cursor_grab(CursorGrabMode::None);
            }
        }
        if ui && self.mouse.texture.is_none() {
            let root = self.front.as_ref().map(|f| f.oni_root.clone()).or_else(|| {
                self.play
                    .as_ref()?
                    .flow
                    .as_ref()
                    .map(|f| f.oni_root.clone())
            });
            self.mouse.texture = Some(root.and_then(|r| arrow_texture(&r.join("common"))));
        }
        self.mouse.arrow = ui
            && !grab
            && self.mouse.inside
            && self.mouse.texture.as_ref().is_some_and(Option::is_some);
        let hidden = grab || self.mouse.arrow;
        if hidden != self.mouse.hidden {
            self.mouse.hidden = hidden;
            self.window.set_cursor_visible(!hidden);
        }
    }

    /// The arrow over everything, centred on the pointer.
    pub(super) fn draw_arrow(&mut self) {
        if !self.mouse.arrow {
            return;
        }
        let Some(Some(path)) = self.mouse.texture.clone() else {
            return;
        };
        let [x, y] = self.cursor;
        let (l, t) = (x - ARROW_CENTER, y - ARROW_CENTER);
        self.renderer.overlay.push(crate::render::OverlayDraw {
            texture: Some(path),
            tris: super::quad([l, t, l + ARROW, t + ARROW]),
            color: [1.0; 4],
            additive: false,
        });
    }
}
