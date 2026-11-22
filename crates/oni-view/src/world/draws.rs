//! What the world shows this frame, for the renderer (`crate::draw`'s
//! plain data): sprites, contrails and lens flares, decals, the one-bone
//! models (guns, particles' models, turrets, powerups, placed objects,
//! consoles), corpses and magic effects. Read-only: building a frame never
//! changes the world.

use std::path::PathBuf;

use glam::{Mat3, Mat4, Vec3};

use super::magic::{INVIS_AURA, PARTS, SHELL_SCALE};
use super::objects::oflag;
use super::weapons::{GunSight, LASER_WIDTH};
use super::*;
use crate::p3;

impl World {
    /// What to draw for the powerups lying about: each one's model, turned.
    pub fn powerup_draws(&self) -> Vec<(std::path::PathBuf, Mat4)> {
        self.armory
            .powerups
            .iter()
            .filter_map(|p| {
                let model = self.armory.powerup_looks.get(p.kind)?.model.clone()?;
                Some((
                    model,
                    Mat4::from_rotation_translation(
                        glam::Quat::from_rotation_y(p.rotation),
                        p.position,
                    ),
                ))
            })
            .collect()
    }

    /// The glows lying under the powerups: flat sprites turned with them,
    /// at a third of full brightness, drawn without writing depth
    /// (`WPrPowerup_Draw`).
    fn powerup_glows(&self) -> impl Iterator<Item = crate::draw::Sprite> + '_ {
        self.armory.powerups.iter().filter_map(|p| {
            let look = self.armory.powerup_looks.get(p.kind)?;
            let texture = look.glow.clone()?;
            let turn = Mat3::from_rotation_y(p.rotation);
            let bright = 1.0 / 3.0;
            Some(crate::draw::Sprite {
                at: p.position + Vec3::Y * look.glow_height,
                size: look.glow_size,
                mode: crate::draw::SpriteMode::Fixed([turn.x_axis, turn.z_axis]),
                rotation: 0.0,
                color: [bright; 3],
                texture,
                contrail: false,
                no_depth: false,
            })
        })
    }

    /// What to draw of the guns: each one's model and world matrix (guns
    /// in hands, and guns lying about; holstered ones are not shown).
    pub fn gun_draws(&self) -> Vec<(std::path::PathBuf, Mat4)> {
        let holstered: std::collections::HashSet<usize> = self
            .fighters
            .iter()
            .flat_map(|f| [f.holster, f.magic])
            .flatten()
            .collect();
        self.armory
            .weapons
            .iter()
            .enumerate()
            .filter(|(i, g)| {
                !g.gone
                    && !holstered.contains(i)
                    && g.owner.is_none_or(|o| {
                        self.fighters
                            .get(o)
                            .is_some_and(|f| f.hand == Some(*i) && f.effects.body_alpha() >= 1.0)
                    })
            })
            .filter_map(|(_, g)| Some((g.class.model.clone()?, g.matrix.get())))
            .collect()
    }

    /// The guns in the hands of bodies fading (invisibility): drawn with
    /// the body, at its alpha (the original's weapon is part of the body
    /// drawn); none once it is gone.
    pub fn faded_gun_draws(&self) -> Vec<(std::path::PathBuf, Mat4, f32)> {
        self.fighters
            .iter()
            .filter_map(|f| {
                let alpha = f.effects.body_alpha();
                let g = self.armory.weapons.get(f.hand?)?;
                (alpha < 1.0 && alpha > 0.0 && !g.gone)
                    .then(|| Some((g.class.model.clone()?, g.matrix.get(), alpha)))
                    .flatten()
            })
            .collect()
    }

    /// The particles that are models (superballs, grenades, darts, shells,
    /// casings): each one's model and world matrix — its position, its
    /// orientation, its class's scale.
    pub fn particle_models(&self) -> Vec<(std::path::PathBuf, Mat4)> {
        use p3::class::{cflag, Value};
        let sys = &self.armory.particles;
        let models = self.level_dir.join("../../common/models");
        sys.iter()
            .filter(|(_, p)| p.flags & p3::pflag::HIDDEN == 0)
            .filter_map(|(_, p)| {
                let c = sys.class(p.class);
                if !c.has(cflag::GEOMETRY) || c.has(cflag::INVISIBLE) || c.instance.is_empty() {
                    return None;
                }
                let scale = match c.scale {
                    Value::Float(x) if x > 0.0 => x,
                    _ => 1.0,
                };
                let m = Mat4::from_mat3_translation(
                    sys.world_orientation(p) * scale,
                    sys.world_position(p),
                );
                Some((models.join(format!("{}.glb", c.instance)), m))
            })
            .collect()
    }

    /// The particles to draw, as sprites: each class's texture at its
    /// scale — facing the camera, or stretched along its flight for the
    /// directional sprite kinds. (Models, contrails' trails, tint and fading
    /// aren't drawn yet.)
    /// The decals to draw from `eye` (`docs/PARTICLES.md`, *Decals*): the
    /// level's, then the rest, oldest first — each only while the quad it
    /// was laid on is drawn (not one-sided and seen from behind), at the
    /// quad's see-through opacity while it is see-through, full otherwise
    /// (`P3iDisplayDecals`). Drawn opacity: the decal's times that, doubled,
    /// at most full (the original's draw doubles it).
    pub fn decal_draws(&self, eye: Vec3) -> Vec<crate::draw::DecalDraw> {
        use crate::quads::flag;
        let textures = self.level_dir.join("../../common/textures");
        let quads = &self.quads;
        // The quad's opacity this frame; none while it isn't drawn.
        let quad_alpha = |quad: Option<u32>| -> Option<u32> {
            let Some(q) = quad.and_then(|q| quads.quads.get(q as usize)) else {
                return Some(255);
            };
            if q.flags & flag::AK_2SIDED == 0 {
                let (n, _) = quads.plane(q.plane);
                if (eye - quads.corner(q, 0)).dot(n) < 0.0 {
                    return None;
                }
            }
            Some(if q.flags & flag::JELLO != 0 {
                super::camera::JELLO_AMT
            } else {
                255
            })
        };
        let draw = |mesh: Rc<crate::decal::Mesh>, alpha: u16, texture: &str| {
            let shown = quad_alpha(mesh.quad)?;
            Some(crate::draw::DecalDraw {
                positions: mesh,
                alpha: ((u32::from(alpha) * shown * 2) >> 8).min(255) as f32 / 255.0,
                texture: textures.join(format!("{texture}.png")),
                additive: self.additive_textures.contains(texture),
            })
        };
        let decals = &self.armory.particles.decals;
        // The level's, then the characters' shadows (laid again each frame,
        // so after them), then the rest.
        let mut out: Vec<_> = decals
            .fixed
            .iter()
            .filter_map(|d| draw(d.mesh.clone(), d.alpha, &d.texture))
            .collect();
        for k in 0..self.fighters.len() {
            if let Some((mesh, alpha, texture)) = self.shadow(k) {
                out.extend(draw(Rc::new(mesh), alpha, &texture));
            }
        }
        out.extend(
            decals
                .dynamic
                .iter()
                .filter_map(|d| draw(d.mesh.clone(), d.alpha, &d.texture)),
        );
        out
    }

    /// The texture particle `p` shows now: its class's, or — animated — the
    /// frame showing, its time from the particle's birth (with a texture
    /// time) or the game's, started at its random index (with a texture
    /// index; `M3iResolveTexture`).
    pub fn particle_texture(&self, p: &p3::Particle) -> std::path::PathBuf {
        use p3::class::cflag;
        let sys = &self.armory.particles;
        let c = sys.class(p.class);
        match self.texture_anims.get(&c.instance) {
            Some(a) => {
                let time = if c.has(cflag::TEXTURE_TIME) {
                    sys.tick.wrapping_sub(p.texture_time)
                } else {
                    sys.tick
                };
                let instance = if c.has(cflag::TEXTURE_INDEX) {
                    p.texture_index
                } else {
                    0
                };
                a.file(time, instance).to_path_buf()
            }
            None => self
                .level_dir
                .join("../../common/textures")
                .join(format!("{}.png", c.instance)),
        }
    }

    pub fn sprites(&self) -> Vec<crate::draw::Sprite> {
        use p3::class::{cflag, Value};
        let sys = &self.armory.particles;
        let textures = self.level_dir.join("../../common/textures");
        let read = |v: &Value, vars: &[p3::Val]| -> Option<f32> {
            match v {
                Value::Float(x) => Some(*x),
                Value::Range(a, b) => Some((a + b) / 2.0),
                Value::Bell(m, _) => Some(*m),
                Value::Var(i) => vars.get(*i)?.f(),
                _ => None,
            }
        };
        sys.iter()
            .filter(|(_, p)| p.flags & p3::pflag::HIDDEN == 0)
            .filter_map(|(h, p)| {
                let c = sys.class(p.class);
                // (Decals are drawn as decals; contrails as their links.)
                if c.has(cflag::INVISIBLE)
                    || c.has(cflag::GEOMETRY)
                    || c.has(cflag::DECAL)
                    || c.instance.is_empty()
                    || c.sprite_type.is_contrail()
                {
                    return None;
                }
                let x = read(&c.scale, &p.vars)?;
                let y = c
                    .y_scale
                    .as_ref()
                    .and_then(|v| read(v, &p.vars))
                    .unwrap_or(x);
                // How it lies (`MSrGeomContext_Method_Sprite_Draw`): its
                // direction its orientation's Y, else its velocity; the
                // fixed kinds from its orientation's axes.
                let o = c.has(cflag::ORIENTATION).then(|| sys.world_orientation(p));
                let dir = o.map_or(p.velocity, |o| o.y_axis);
                use crate::draw::SpriteMode;
                use crate::p3::class::SpriteType as T;
                let mode = match (c.sprite_type, o) {
                    (T::Rotated, _) => SpriteMode::Rotated(dir),
                    (T::Billboard | T::ContrailOriented | T::Contrail, _) => {
                        SpriteMode::Billboard(dir)
                    }
                    // Arrow: along its direction, upright along its up.
                    (T::Arrow, Some(o)) => SpriteMode::Fixed([o.y_axis, o.z_axis]),
                    // Flat: across its direction.
                    (T::Flat, Some(o)) => SpriteMode::Fixed([o.x_axis, o.z_axis]),
                    // Discus: along its direction and its right.
                    (T::Discus, Some(o)) => SpriteMode::Fixed([o.y_axis, o.x_axis]),
                    _ => SpriteMode::Facing,
                };
                let rotation = read(&c.rotation, &p.vars).unwrap_or(0.0);
                // Its tint (ARGB) and alpha: additive, so fainter is darker.
                let shade = match &c.tint {
                    Value::Shade(s) | Value::ShadeRange(s, _) | Value::ShadeBell(s, _) => *s,
                    Value::Var(i) => match p.vars.get(*i) {
                        Some(p3::Val::U(u)) => *u,
                        _ => 0xFFFF_FFFF,
                    },
                    _ => 0xFFFF_FFFF,
                };
                // (A lens flare: as far as it is seen; over everything.)
                let lens = c.lens.as_ref().map(|_| self.lens_alpha(h).unwrap_or(0.0));
                if lens == Some(0.0) {
                    return None;
                }
                // (A fading flare fades with the fog, as a thing at its depth
                // would: `P3rModifyAlphaByFogFromDepth`.)
                let fog = match c.lens.as_ref() {
                    Some(l) if l.fade.is_some() => self.flare_fog(sys.world_position(p)),
                    _ => 1.0,
                };
                if fog <= 0.0 {
                    return None;
                }
                let alpha = read(&c.alpha, &p.vars).unwrap_or(1.0).clamp(0.0, 1.0)
                    * lens.unwrap_or(1.0)
                    * fog;
                let channel = |shift: u32| ((shade >> shift) & 0xFF) as f32 / 255.0 * alpha;
                Some(crate::draw::Sprite {
                    at: sys.world_position(p),
                    size: [x, y],
                    mode,
                    rotation,
                    color: [channel(16), channel(8), channel(0)],
                    texture: self.particle_texture(p),
                    contrail: false,
                    no_depth: lens.is_some(),
                })
            })
            .chain(
                (0..self.fighters.len())
                    .filter_map(|who| self.gun_sight(who).map(|s| (who, s)))
                    .flat_map(|(who, s)| self.sight_sprites(who, &s, &textures)),
            )
            .chain(self.powerup_glows())
            .chain(self.contrails())
            .collect()
    }

    /// The contrails (`P3rDisplayClass_Contrail`, `M3rContrail_Draw`): each
    /// particle of a contrail class joined to the one it links to by a
    /// band — across its X (`Contrail`, sprite type 5) or square to the
    /// link and the line of sight (`ContrailRotated`, 6) at its scale;
    /// v 0 at it, 1 at the link; its colour at its end, the link's at the
    /// other, both at its alpha. A contrail emitter draws the part of the
    /// band to its newest emission that has grown since it was emitted.
    /// How much of a lens flare at `at` the fog leaves (`gl_calculate_fog_factor`
    /// at its screen depth, linear from the fog's start to its end). As
    /// shipped (Q28) a flare past the end is left whole.
    fn flare_fog(&self, at: Vec3) -> f32 {
        let f = &self.fog.fog;
        let (start, end) = (f.start, f.end);
        if start == end || start >= 1.0 || end <= 0.0 {
            return 1.0;
        }
        let clip = self.camera.render_camera(4.0 / 3.0).view_proj() * at.extend(1.0);
        if clip.w <= 1e-6 {
            return 1.0;
        }
        // (Screen depth as the fog has it: −1 near, 1 far.)
        let z = clip.z / clip.w * 2.0 - 1.0;
        if z <= start {
            1.0
        } else if z >= end {
            if self.quirks.flares_unfogged_past_fog_end() {
                1.0
            } else {
                0.0
            }
        } else {
            (end - z) / (end - start)
        }
    }

    /// How many contrail links are drawn now.
    pub fn contrail_count(&self) -> usize {
        self.contrails().len()
    }

    fn contrails(&self) -> Vec<crate::draw::Sprite> {
        use p3::class::{cflag, Link, Value};
        let sys = &self.armory.particles;
        let cam = self.camera.view.location;
        let read = |v: &Value, vars: &[p3::Val]| -> Option<f32> {
            match v {
                Value::Float(x) => Some(*x),
                Value::Range(a, b) => Some((a + b) / 2.0),
                Value::Bell(m, _) => Some(*m),
                Value::Var(i) => vars.get(*i)?.f(),
                _ => None,
            }
        };
        let tint = |p: &p3::Particle| -> u32 {
            match &sys.class(p.class).tint {
                Value::Shade(s) | Value::ShadeRange(s, _) | Value::ShadeBell(s, _) => *s,
                Value::Var(i) => match p.vars.get(*i) {
                    Some(p3::Val::U(u)) => *u,
                    _ => 0xFFFF_FFFF,
                },
                _ => 0xFFFF_FFFF,
            }
        };
        let live = |h: p3::Handle| {
            sys.get(h)
                .filter(|q| q.flags & p3::pflag::BREAK_LINKS_TO_ME == 0)
        };
        // A particle's half-width at its end of a link toward `to`.
        let width = |p: &p3::Particle, at: Vec3, to: Vec3| -> Option<Vec3> {
            let c = sys.class(p.class);
            let scale = read(&c.scale, &p.vars)?;
            Some(
                if c.sprite_type == p3::class::SpriteType::ContrailOriented {
                    if !c.has(cflag::ORIENTATION) {
                        return None;
                    }
                    sys.world_orientation(p).x_axis * scale
                } else {
                    let w = (to - at).cross(at - cam);
                    let l = w.length();
                    if l > 0.01 {
                        w * (scale / l)
                    } else {
                        w
                    }
                },
            )
        };
        // Its width as the far end of a link: its own (toward its own link),
        // or — first in its chain, or its link gone — the near end's.
        let far_width = |q: &p3::Particle, at: Vec3, near: Vec3| -> Vec3 {
            q.link
                .and_then(live)
                .and_then(|r| width(q, at, sys.world_position(r)))
                .unwrap_or(near)
        };
        let mut out = Vec::new();
        for (_, p) in sys.iter() {
            let c = sys.class(p.class);
            if !c.sprite_type.is_contrail()
                || p.flags & p3::pflag::HIDDEN != 0
                || c.has(cflag::INVISIBLE)
                || c.instance.is_empty()
            {
                continue;
            }
            // What it joins to, and how much of the band shows.
            let (link, start_v) = if c.contrail_emitter {
                let Some(k) = c
                    .emitters
                    .iter()
                    .enumerate()
                    .position(|(k, e)| e.link == Link::LastEmission(k))
                else {
                    continue;
                };
                let Some(st) = p.emitters.get(k) else {
                    continue;
                };
                let Some(l) = st.last.and_then(live) else {
                    continue;
                };
                let interval = read(&c.emitters[k].rate.1[0], &p.vars).unwrap_or(1.0);
                let since = (p.time - st.last_time) as f32;
                let v = if since < 0.0 {
                    1.0
                } else if since > interval {
                    0.0
                } else {
                    1.0 - since / interval
                };
                (l, v)
            } else {
                let Some(l) = p.link.and_then(live) else {
                    continue;
                };
                (l, 0.0)
            };
            let (at, to) = (sys.world_position(p), sys.world_position(link));
            let along = to - at;
            if c.max_contrail * c.max_contrail > 0.001
                && along.length_squared() > c.max_contrail * c.max_contrail
            {
                continue;
            }
            let Some(w0) = width(p, at, to) else { continue };
            let w1 = far_width(link, to, w0);
            // Its alpha: fading out, its life left; else its appearance's.
            let mut alpha = if p.flags & p3::pflag::FADE_OUT != 0 && p.fadeout_time > 0.0 {
                (p.lifetime / p.fadeout_time).clamp(0.0, 1.0)
            } else {
                read(&c.alpha, &p.vars).unwrap_or(1.0).clamp(0.0, 1.0)
            };
            if let Some((lo, hi)) = c.edge_fade {
                let edge = 1.0
                    - along
                        .normalize_or_zero()
                        .dot((at - cam).normalize_or_zero())
                        .abs();
                if edge < lo {
                    continue;
                } else if edge < hi {
                    alpha *= (edge - lo) / (hi - lo);
                }
            }
            let rgb = |shade: u32| {
                let ch = |shift: u32| ((shade >> shift) & 0xFF) as f32 / 255.0 * alpha;
                [ch(16), ch(8), ch(0)]
            };
            let (c0, c1) = (rgb(tint(p)), rgb(tint(link)));
            out.push(crate::draw::Sprite {
                at,
                size: [0.0, 0.0],
                mode: crate::draw::SpriteMode::Quad([
                    (at + w0, [0.0, start_v], c0),
                    (at - w0, [1.0, start_v], c0),
                    (to - w1, [1.0, 1.0], c1),
                    (to + w1, [0.0, 1.0], c1),
                ]),
                rotation: 0.0,
                color: c0,
                texture: self.particle_texture(p),
                contrail: true,
                no_depth: false,
            });
        }
        out
    }

    /// The sprites drawing a gun's sight (`ONiDrawLaserSight`,
    /// `ONiDrawTunnel`): the laser, a camera-facing contrail 0.2 to each
    /// side in the laser's colour (`laser_contrail`, additive); the tunnel's
    /// sprites at 5 × their scale, in their shade.
    fn sight_sprites(
        &self,
        who: usize,
        s: &GunSight,
        textures: &std::path::Path,
    ) -> Vec<crate::draw::Sprite> {
        let mut out = Vec::new();
        let along = s.to - s.from;
        if let Some(color) = s.laser.filter(|_| along.length() > 1e-3) {
            out.push(crate::draw::Sprite {
                at: (s.from + s.to) * 0.5,
                size: [along.length() * 0.5, LASER_WIDTH],
                mode: crate::draw::SpriteMode::Billboard(along),
                rotation: 0.0,
                color,
                texture: textures.join("laser_contrail.png"),
                contrail: true,
                no_depth: false,
            });
        }
        let class = self.fighters[who]
            .hand
            .map(|w| &self.armory.weapons[w].class);
        if let Some(t) = class.and_then(|c| c.tunnel.as_ref()) {
            for &at in &s.tunnel {
                let size = 5.0 * t.scale;
                out.push(crate::draw::Sprite {
                    at,
                    size: [size, size],
                    mode: crate::draw::SpriteMode::Facing,
                    rotation: 0.0,
                    color: t.color,
                    texture: textures.join(format!("{}.png", t.texture)),
                    contrail: false,
                    no_depth: false,
                });
            }
        }
        // The player's reticle (`ONrGameState_DisplayAiming`, `WPrDrawSprite`):
        // at the sight's end — the targeted one on a character — sized by
        // the class: a fixed 5, 0.7 × distance^0.3 (half scale), or 0.03 ×
        // distance (no scale), times its own scale.
        if let Some(c) = class.filter(|_| who == 0) {
            let sprite = if s.target == SightTarget::Character {
                c.cursor_targeted.as_ref()
            } else {
                c.cursor.as_ref()
            };
            if let Some(sp) = sprite {
                let d = self.camera.view.location.distance(s.to);
                let size = if c.has(crate::weapons::flag::NO_SCALE) {
                    0.03 * d
                } else if c.has(crate::weapons::flag::HALF_SCALE) {
                    0.7 * d.powf(0.3)
                } else {
                    5.0
                } * sp.scale;
                out.push(crate::draw::Sprite {
                    at: s.to,
                    size: [size * 0.5, size * 0.5],
                    mode: crate::draw::SpriteMode::Facing,
                    rotation: 0.0,
                    color: sp.color,
                    texture: textures.join(format!("{}.png", sp.texture)),
                    contrail: false,
                    no_depth: false,
                });
            }
        }
        out
    }

    /// What to draw for the objects: each shown one's models and matrix —
    /// all but those see-through this frame ([`World::object_jello_draws`]).
    /// The objects to draw: each model, its matrix, and the flat shade
    /// it is lit by, if any (`obj_shade`, `OBcFlags_FlatLighting`).
    pub fn object_draws(&self) -> Vec<(PathBuf, Mat4, Option<[f32; 3]>)> {
        // (The laser triggers' emitter eyes are drawn as objects too.)
        let eyes = self
            .lasers
            .iter()
            .flat_map(|l| l.eye_draws())
            .map(|(p, m)| (p, m, None));
        (0..self.placed.list.len())
            .filter(|&k| {
                self.placed.list[k].flags & oflag::INVISIBLE == 0
                    && !self.see_through.objects.contains(&k)
            })
            .flat_map(|k| {
                let m = self.object_matrix(k);
                let shade = self.placed.list[k].shade;
                self.placed.setups[self.placed.list[k].setup]
                    .models
                    .iter()
                    .map(move |p| (p.clone(), m, shade))
            })
            .chain(eyes)
            .collect()
    }

    /// The objects see-through this frame (the camera's jello: drawn at
    /// 31 of 255, `OBrDraw`).
    pub fn object_jello_draws(&self) -> Vec<(PathBuf, Mat4)> {
        self.object_models(true)
    }

    /// What to draw of the turrets: each head and barrel model and matrix.
    pub fn turret_draws(&self) -> Vec<(PathBuf, Mat4)> {
        self.turrets
            .iter()
            .flat_map(|t| {
                [
                    t.class.head.clone().map(|p| (p, t.head_matrix())),
                    t.class.barrel.clone().map(|p| (p, t.barrel_matrix())),
                ]
            })
            .flatten()
            .collect()
    }
    /// What to draw of the consoles: each screen's model, its texture by
    /// the console's state (inactive, active, used; none set: `NONE`), and
    /// the console's matrix.
    pub fn console_draws(&self) -> Vec<(std::path::PathBuf, std::path::PathBuf, glam::Mat4)> {
        let common = self.level_dir.join("../../common");
        self.consoles
            .iter()
            .filter_map(|c| {
                let model = common.join(c.screen_model.as_ref()?);
                let name = Self::screen_of(c);
                Some((
                    model,
                    common.join("textures").join(format!("{name}.png")),
                    glam::Mat4::from_mat3_translation(c.rotation, c.position),
                ))
            })
            .collect()
    }

    /// The corpses to draw: each its model and parts' matrices.
    pub fn corpse_draws(&self) -> impl Iterator<Item = crate::draw::CharDraw<'_>> {
        self.corpses.list.iter().filter_map(|c| {
            Some(crate::draw::CharDraw {
                model: c.model?,
                bones: &c.bones,
                see_through: false,
                alpha: 1.0,
                shade: None,
            })
        })
    }
    /// The magic effects to draw this frame (`ONrCharacter_Draw`), given
    /// each fighter's body matrices: over its low body, each part scaled up
    /// 1.4 about its origin — the shield's glow part by part, then
    /// invisibility's shimmer, as bright as the body is faded out (at most
    /// 22%).
    pub fn magic_draws(&self, bones: &[Vec<glam::Mat4>]) -> Vec<crate::draw::MagicDraw> {
        use crate::draw::MagicKind;
        let time = self.now as u32;
        let mut out = Vec::new();
        for (f, b) in self.fighters.iter().zip(bones) {
            let Some(model) = f.magic_model else { continue };
            if f.gone || (!f.effects.shield_active && !f.effects.invis_active) {
                continue;
            }
            let scaled: Vec<glam::Mat4> = b
                .iter()
                .map(|m| *m * glam::Mat4::from_scale(Vec3::splat(SHELL_SCALE)))
                .collect();
            if f.effects.shield_active {
                // (A super shield's and a boss's red.)
                let (kind, tex) = if f.effects.daodan || f.boss_shield {
                    (MagicKind::Daodan, 1)
                } else {
                    (MagicKind::Shield, 0)
                };
                if let Some(texture) = self.magic_textures[tex].clone() {
                    let mut alpha = [0.0; PARTS];
                    for (a, p) in alpha.iter_mut().zip(f.effects.shield_parts) {
                        *a = f32::from(p) / 255.0;
                    }
                    out.push(crate::draw::MagicDraw {
                        model,
                        bones: scaled.clone(),
                        texture,
                        kind,
                        alpha,
                        time,
                    });
                }
            }
            if f.effects.invis_active {
                if let Some(texture) = self.magic_textures[2].clone() {
                    let a = ((1.0 - f.effects.invis_alpha) * 255.0 * INVIS_AURA).round() / 255.0;
                    out.push(crate::draw::MagicDraw {
                        model,
                        bones: scaled,
                        texture,
                        kind: MagicKind::Invis,
                        alpha: [a; PARTS],
                        time,
                    });
                }
            }
        }
        out
    }

    fn object_models(&self, see_through: bool) -> Vec<(PathBuf, Mat4)> {
        (0..self.placed.list.len())
            .filter(|&k| {
                self.placed.list[k].flags & oflag::INVISIBLE == 0
                    && self.see_through.objects.contains(&k) == see_through
            })
            .flat_map(|k| {
                let m = self.object_matrix(k);
                self.placed.setups[self.placed.list[k].setup]
                    .models
                    .iter()
                    .map(move |p| (p.clone(), m))
            })
            .collect()
    }
}
