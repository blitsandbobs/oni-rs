//! Opening a level's world — the steps the play app (menu missions and
//! `--play`) and the scenario host share, so what a scenario runs is what
//! the game runs: the mode and difficulty, navigation, the player as
//! fighter 0, the level's objects, corpses' models, doors, glass, weapons
//! (and with them the level's quads), then the renderer's view of them.

use std::path::Path;

use glam::Vec3;

use crate::classes::Classes;
use crate::gltf::Scene;
use crate::render::{Gpu, Renderer};
use crate::world::CharKind;
use crate::world::World;

/// What to open a level with.
pub struct Opening<'a> {
    pub level_dir: &'a Path,
    pub faithful: bool,
    pub difficulty: &'a str,
    /// The player's kind and where they start (facing +Z).
    pub player: &'a CharKind,
    pub at: Vec3,
}

/// Opens the level's world (`ONrLevel_Load`'s world steps), reporting the
/// level-load dialog's percents as it goes (`progress`).
pub fn open_world(
    gpu: &Gpu,
    renderer: &mut Renderer,
    scene: &Scene,
    classes: &mut Classes,
    o: Opening<'_>,
    progress: &mut dyn FnMut(u32),
) -> World {
    let mut world = World::new();
    if o.faithful {
        world.set_quirks(crate::quirks::Quirks::FAITHFUL);
    }
    world.difficulty = classes.difficulty(o.difficulty);
    progress(30);
    world.nav = crate::nav::Navigation::load(o.level_dir)
        .ok()
        .map(std::rc::Rc::new);
    if world.nav.is_none() {
        eprintln!("  (no navigation data for this level)");
    }
    progress(35);
    // The player is fighter 0.
    world.spawn(o.player, o.at, 0.0, false);
    world.cheats_permitted = crate::game::cheats_permitted();
    world.load_level(o.level_dir);
    world.attach_corpse_models(|c| {
        classes
            .kind(c, |ch| renderer.add_character(gpu, ch))
            .ok()
            .map(|k| k.model)
    });
    progress(55);
    world
        .doors
        .attach(crate::door_meshes(scene), &mut world.quads);
    world.attach_glass(crate::glass_parts(scene));
    progress(65);
    world.attach_weapons(&o.level_dir.join("../../common"));
    // (After the weapons: that is where the level quads load.)
    renderer.attach_quads(scene, &world.quads);
    renderer.attach_texture_anims(gpu, world.texture_anims());
    renderer.attach_sky(gpu, o.level_dir);
    progress(70);
    world
}
