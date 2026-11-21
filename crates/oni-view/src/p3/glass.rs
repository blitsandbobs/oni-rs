//! Breaking glass (`P3iDamageQuad`, `ONrParticle3_EmitGlassPieces`): a pane
//! that breaks bursts into `glass_shard` particles spread over it, thrown
//! by what broke it (`docs/PARTICLES.md`, *Breaking glass*).

use glam::{Mat3, Vec3};

use super::effect::{Data, Location, Orientation, Spec};
use super::{ClassId, Host, System};

/// The pane's cell size and shards per cell, by graphics quality
/// (`ONcGlass_Cellsize`, `ONcGlass_ShardsPerCell`).
const CELL_SIZE: [f32; 3] = [22.0, 20.0, 15.0];
const SHARDS_PER_CELL: [f32; 3] = [2.5, 3.5, 5.0];
const MAX_CELLS: u32 = 50;
const RANDOM_VELOCITY: f32 = 10.0;
const BLAST_DIRECTED_VELOCITY: f32 = 90.0;
const BLAST_DIRECTED_VEL_DAMAGE: f32 = 25.0;
const BLAST_NORMAL_VELOCITY: f32 = 12.0;
const BLAST_OFFSET: f32 = 2.0;
const INTERP_RADIUS_SCALE: f32 = 1.5;
/// How far off the pane's plane a blast still counts as in it
/// (`P3cParallelBlastTolerance`).
const PARALLEL_BLAST_TOLERANCE: f32 = 4.0;

/// What broke a pane by blowing it out: the blast's centre, the way it
/// travelled (if known), its radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blast {
    pub centre: Vec3,
    pub dir: Option<Vec3>,
    pub radius: f32,
}

/// A pane to break: its corners (as the level stores them; a triangle's
/// fourth repeats one) and plane (normal, d).
#[derive(Clone, Copy, Debug)]
pub struct Pane {
    pub corners: [Vec3; 4],
    pub normal: Vec3,
    pub d: f32,
}

impl System {
    /// `P3iDamageQuad`'s shattering: bursts `pane` into `class`'s shards
    /// (`damage`: how hard it was hit; `blast`: what blew it out, none for a
    /// shot). `quality`: the graphics quality, 0–2.
    pub fn break_glass(
        &mut self,
        class: ClassId,
        pane: &Pane,
        damage: f32,
        blast: Option<Blast>,
        quality: usize,
        host: &mut dyn Host,
    ) {
        // The normal faces the blast (two-sided glass would otherwise throw
        // its shards back at it); a blast in the pane's plane blows along
        // the way it came.
        let mut normal = pane.normal;
        let mut dir = None;
        if let Some(b) = blast {
            let side = b.centre.dot(normal) + pane.d;
            if side < -PARALLEL_BLAST_TOLERANCE {
                normal = -normal;
            } else if side <= PARALLEL_BLAST_TOLERANCE {
                if b.dir.is_some_and(|d| d.dot(normal) > 0.0) {
                    normal = -normal;
                }
                dir = b.dir;
            }
        }
        self.emit_glass(
            class,
            pane,
            damage,
            blast.map(|b| (b.centre, b.radius)),
            normal,
            dir,
            quality.min(2),
            host,
        );
    }

    /// `ONrParticle3_EmitGlassPieces`.
    #[allow(clippy::too_many_arguments)]
    fn emit_glass(
        &mut self,
        class: ClassId,
        pane: &Pane,
        damage: f32,
        blast: Option<(Vec3, f32)>,
        normal: Vec3,
        dir: Option<Vec3>,
        quality: usize,
        host: &mut dyn Host,
    ) {
        // (Not thrown absurdly fast; no direction: straight through.)
        let damage = if blast.is_some() {
            damage.max(25.0)
        } else {
            damage
        };
        let dir = dir.unwrap_or(-normal);
        let p = pane.corners;
        let side_x0 = p[1] - p[0];
        let side_x1 = p[2] - p[3];
        let side_z = p[3] - p[0];
        let (length_x0, length_x1, length_z) =
            (side_x0.length(), side_x1.length(), side_z.length());
        // (Degenerate or almost collinear: nothing.)
        if length_x0 < 1e-3 {
            return;
        }
        let orient_x = side_x0 / length_x0;
        let orient_y = side_z.cross(orient_x);
        if orient_y.length() < 1e-3 {
            return;
        }
        let orient_y = orient_y.normalize();
        let orient_z = orient_x.cross(orient_y);
        if orient_z.length() < 1e-3 {
            return;
        }
        let orientation = Mat3::from_cols(orient_x, orient_y, orient_z.normalize());

        let cell = CELL_SIZE[quality];
        let cells_x = ((length_x0 / cell) as u32 + 1).min(MAX_CELLS);
        let cells_z = ((length_z / cell) as u32 + 1).min(MAX_CELLS);
        let (step_x0, step_x1, step_z) = (
            side_x0 / cells_x as f32,
            side_x1 / cells_x as f32,
            side_z / cells_z as f32,
        );
        let spec = Spec {
            class,
            orientation: Orientation::Unchanged,
            location: Location::Default,
        };
        for z in 0..cells_z {
            // A trapezoid's rows get as many shards as their width holds.
            let mut density = SHARDS_PER_CELL[quality];
            if cells_z > 1 {
                let interp = z as f32 / (cells_z - 1) as f32;
                density *= (1.0 - interp) + interp * (length_x1 / length_x0);
            }
            for x in 0..cells_x {
                let mut n = density.round().max(0.0) as u32;
                if self.random() < density - n as f32 {
                    n += 1;
                }
                for _ in 0..n {
                    let pos_x = x as f32 + self.random();
                    let pos_z = z as f32 + self.random();
                    let exact = pos_z / cells_z as f32;
                    let at = p[0]
                        + step_z * pos_z
                        + step_x0 * (pos_x * (1.0 - exact))
                        + step_x1 * (pos_x * exact);
                    self.glass_piece(&spec, orientation, at, damage, blast, normal, dir, host);
                }
            }
        }
    }

    /// `ONiParticle3_EmitGlassPiece`: one shard at `at`, a little random
    /// speed, and blown out by a blast — near its centre along the blast,
    /// toward its edge out along the pane's normal.
    #[allow(clippy::too_many_arguments)]
    fn glass_piece(
        &mut self,
        spec: &Spec,
        orientation: Mat3,
        at: Vec3,
        damage: f32,
        blast: Option<(Vec3, f32)>,
        normal: Vec3,
        dir: Vec3,
        host: &mut dyn Host,
    ) {
        let mut data = Data::new(self.tick, at);
        data.parent_orientation = Some(orientation);
        let speed = RANDOM_VELOCITY * self.random();
        data.velocity = self.random_unit() * speed;
        if let Some((centre, radius)) = blast {
            let away = at - centre;
            let distance = away.length();
            let direction = (away + dir * (BLAST_OFFSET * radius)).normalize_or_zero();
            let outwards = (distance / (radius * INTERP_RADIUS_SCALE)).min(1.0);
            let speed = BLAST_NORMAL_VELOCITY * outwards
                + BLAST_DIRECTED_VELOCITY * (damage / BLAST_DIRECTED_VEL_DAMAGE) * (1.0 - outwards);
            data.velocity += (direction * (1.0 - outwards) - normal * outwards) * speed;
        }
        if self.create_effect(spec, &data, host).is_none() {
            eprintln!(
                "### glass emitting {}: unable to spawn particle",
                self.classes.list[spec.class].name
            );
        }
    }
}
