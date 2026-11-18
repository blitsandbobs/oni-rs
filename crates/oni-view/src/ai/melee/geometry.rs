//! Hit prediction from animation reach (behaviour per `docs/MELEE_AI.md`,
//! *Hit prediction*): would an attack clip reach a target at all
//! ([`bounds`]), and exactly when does it land ([`intersect`])?
//!
//! Everything is in the **attacker's frame** at the attack's start: the
//! attacker at the origin facing +Z, +X to its left, Y up (the clips' own
//! character-local frame).

use glam::{Vec2, Vec3};

use crate::skel::Clip;

/// A target as the attacker sees it.
#[derive(Clone, Copy)]
pub struct Target<'a> {
    /// Its feet, in the attacker's frame.
    pub pos: Vec3,
    /// Its movement per tick, in the attacker's frame.
    pub velocity: Vec3,
    /// Its body cylinder's radius and height span (above its feet).
    pub radius: f32,
    pub height: (f32, f32),
    /// The clip it plays and how far into it (its body track gives the body's
    /// span tick by tick in [`intersect`]).
    pub clip: Option<&'a Clip>,
    pub tick: u16,
}

/// The attacker's side: the clip, the tick to start from, and — in the air —
/// its velocity, the gravity and how long the jump is expected to last.
#[derive(Clone, Copy)]
pub struct Attacker<'a> {
    pub clip: &'a Clip,
    pub tick: u16,
    pub air: Option<Air>,
    /// Its own body sphere (3 for an AI, 4 for the player).
    pub sphere: f32,
    /// Reproduce the original's slips (`crate::quirks`).
    pub quirks: crate::quirks::Quirks,
}

#[derive(Clone, Copy, Debug)]
pub struct Air {
    pub velocity: Vec3,
    pub gravity: f32,
    pub expected_ticks: u16,
}

/// Why [`intersect`] found no hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Miss {
    /// The clip has no strike left.
    NoAttack,
    AboveHead,
    BelowFeet,
    /// Pointing the wrong way.
    Angle,
    /// Too short.
    Distance,
    /// The target is behind the strike.
    Behind,
}

/// How much of the reach is kept back when planning our own attack.
const SAFETY_MARGIN: f32 = 3.0;
/// Overlap a strike's height band needs with the target's body.
const HEIGHT_OVERLAP: f32 = 1.0;

fn flat(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// Could `attacker`'s clip reach `target` at all (from its start), within
/// the strikes it has left? `planning`: judging our own attack (a safety
/// margin comes off the reach).
pub fn bounds(attacker: &Attacker, target: &Target, planning: bool) -> bool {
    let Some(reach) = &attacker.clip.reach else {
        return false; // a throw, or not an attack
    };
    let Some(last) = reach.strikes.last().map(|s| s.tick) else {
        return false;
    };
    if last < attacker.tick {
        return false;
    }
    let mut n = f32::from(last - attacker.tick);
    let (p, v, r) = (flat(target.pos), flat(target.velocity), target.radius);

    if let Some(air) = attacker.air {
        // Airborne: does the relative path come within reach in time?
        n = n.max(f32::from(air.expected_ticks));
        let reach_r = r + reach.max_distance;
        let w = v - flat(air.velocity);
        let (a, b, c) = (w.dot(w), 2.0 * p.dot(w), p.dot(p) - reach_r * reach_r);
        if c < 0.0 {
            return true;
        }
        if a < 1e-6 {
            return false;
        }
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return false;
        }
        let t0 = (-b - disc.sqrt()) / (2.0 * a);
        if t0 > n {
            return false;
        }
        if t0 < 0.01 && c / (a * t0) < 0.0 {
            return false;
        }
        return true;
    }

    // On the ground: the target's path p → e must come within reach…
    let e = p + v * n;
    let reject = (reach.max_distance + r).powi(2);
    if p.length_squared() > reject && e.length_squared() > reject {
        let (pv, ev) = (p.dot(v), e.dot(v));
        if pv * ev > 0.0 || (ev - pv).abs() < 1e-3 {
            return false;
        }
        let d = e - p;
        let t = (-p.dot(d) / d.length_squared()).clamp(0.0, 1.0);
        if (p + d * t).length_squared() > reject {
            return false;
        }
    }
    // …at the right height…
    if reach.min_height > target.pos.y + target.height.1 {
        return false; // above its head
    }
    if reach.max_height < target.pos.y + target.height.0 {
        return false; // below its feet
    }
    // …and in a direction the strikes actually reach.
    let margin = if planning { SAFETY_MARGIN } else { 0.0 };
    for (i, &spoke) in reach.spokes.iter().enumerate() {
        let len = spoke - margin;
        if len < 0.01 {
            continue;
        }
        let a = (i as f32 * 10.0).to_radians();
        let (along, across) = (Vec2::new(a.sin(), a.cos()), Vec2::new(a.cos(), -a.sin()));
        let (px, py) = (p.dot(along), p.dot(across));
        let (ex, ey) = (e.dot(along), e.dot(across));
        let touches = |x: f32, y: f32| {
            y.abs() < r && {
                let half = (r * r - y * y).sqrt();
                x + half >= 0.0 && x - half <= len
            }
        };
        if touches(px, py) || touches(ex, ey) {
            return true;
        }
        if (py < 0.0) != (ey < 0.0) {
            let x = px + (ex - px) * (py / (py - ey));
            if x > -r && x < len + r {
                return true;
            }
        }
    }
    false
}

/// Steps both bodies forward tick by tick until one of the attacker's
/// strikes lands on the target: the ticks until then, or why it misses.
pub fn intersect(attacker: &Attacker, target: &Target) -> Result<u16, Miss> {
    let clip = attacker.clip;
    let Some(reach) = &clip.reach else {
        return Err(Miss::NoAttack);
    };
    let Some(mut next) = reach.strikes.iter().position(|s| s.tick >= attacker.tick) else {
        return Err(Miss::NoAttack);
    };
    let r = target.radius;
    let touch2 = (attacker.sphere + r).powi(2);
    let dir = if clip.move_direction != 0 {
        clip.move_direction
    } else {
        reach.direction
    };
    let duration = clip.root_motion.len().max(clip.body.len()) as u16;
    let mut counts = [0u32; 5]; // above, below, angle, distance, behind (tie order)
    let (mut a, mut t) = (Vec3::ZERO, target.pos);
    let mut air = attacker.air;
    let mut tick = attacker.tick;
    let mut target_tick = target.tick;
    let body_of = |c: Option<&Clip>, k: u16| {
        c.and_then(|c| c.body.get(usize::from(k)).copied())
            .map_or((0.0, 15.0), |b| (b[2], b[2] + b[3]))
    };
    loop {
        let mut struck = false;
        if reach.strikes[next].tick == tick {
            struck = true;
            let s = reach.strikes[next];
            let (lo, hi) = (s.min_height + a.y - t.y, s.max_height + a.y - t.y);
            let (feet, head) = body_of(target.clip, target_tick);
            if hi < feet + HEIGHT_OVERLAP {
                counts[1] += 1;
            } else if lo > head - HEIGHT_OVERLAP {
                counts[0] += 1;
            } else {
                // A ray from the attacker along the strike's bearing (the
                // original quantises it to 256 sectors).
                let step = std::f32::consts::TAU / 65535.0;
                let sector = ((s.angle / step).round() as u32 >> 8) & 0xFF;
                let ang = (sector * 256 + 128) as f32 * step;
                let d = Vec2::new(ang.sin(), ang.cos());
                let u = flat(a) - flat(t);
                let c = u.length_squared() - r * r;
                if c < 0.0 {
                    return Ok(tick - attacker.tick);
                }
                let b = 2.0 * u.dot(d);
                if b > 0.0 {
                    counts[4] += 1;
                } else {
                    let disc = b * b - 4.0 * c;
                    if disc < 0.0 {
                        counts[2] += 1;
                    } else if (-b - disc.sqrt()) / 2.0 < s.distance {
                        return Ok(tick - attacker.tick);
                    } else {
                        counts[3] += 1;
                    }
                }
            }
            next += 1;
            if next == reach.strikes.len() {
                return Err(most(counts));
            }
        }

        // Advance the target (steady velocity) and the attacker (its clip's
        // root motion, or its flight).
        t += target.velocity;
        match air.as_mut() {
            Some(f) => {
                a += f.velocity;
                f.velocity.y -= f.gravity;
            }
            None => {
                if let Some(&[x, z]) = clip.root_motion.get(usize::from(tick)) {
                    a += Vec3::new(x, 0.0, z);
                }
            }
        }
        tick += 1;
        if tick + 1 >= duration {
            return Err(most(counts));
        }
        let target_len = target.clip.map_or(0, |c| c.body.len() as u16);
        if target_tick + 1 < target_len {
            target_tick += 1;
        }

        // Bodies touching between strikes: the attacker is held back along
        // its attack axis.
        if !struck {
            let d = flat(a) - flat(t);
            let (my_feet, my_head) = body_of(Some(clip), tick);
            let (its_feet, its_head) = body_of(target.clip, target_tick);
            let head_twice = if attacker.quirks.contact_head_twice() { t.y } else { 0.0 };
            let overlap = a.y + my_feet < t.y + its_head + head_twice && t.y + its_feet < a.y + my_head;
            if d.length_squared() <= touch2 && overlap {
                match dir {
                    2 => a.z = t.z + (touch2 - d.x * d.x).max(0.0).sqrt(), // back
                    3 => a.x = t.x - (touch2 - d.y * d.y).max(0.0).sqrt(), // left
                    4 => a.x = t.x + (touch2 - d.y * d.y).max(0.0).sqrt(), // right
                    _ => a.z = t.z - (touch2 - d.x * d.x).max(0.0).sqrt(), // forward
                }
            }
        }
    }
}

/// The most frequent miss (ties: above head, below feet, angle, distance,
/// behind).
fn most(counts: [u32; 5]) -> Miss {
    const ORDER: [Miss; 5] = [Miss::AboveHead, Miss::BelowFeet, Miss::Angle, Miss::Distance, Miss::Behind];
    let best = counts.iter().copied().max().unwrap_or(0);
    if best == 0 {
        return Miss::NoAttack;
    }
    ORDER[counts.iter().position(|&c| c == best).unwrap_or(0)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::collection::tests::konoko;

    fn clip<'a>(c: &'a crate::anim::collection::AnimCollection, name: &str) -> &'a Clip {
        &c.entries.iter().find(|e| e.name == name).expect(name).clip
    }

    fn standing(pos: Vec3) -> Target<'static> {
        Target {
            pos,
            velocity: Vec3::ZERO,
            radius: 3.0,
            height: (0.0, 17.0),
            clip: None,
            tick: 0,
        }
    }

    #[test]
    fn a_jab_reaches_ahead_only() {
        let Some(c) = konoko() else { return };
        let jab = Attacker { clip: clip(&c, "KONCOMcomb_p"), tick: 0, air: None, sphere: 3.0, quirks: Default::default() };
        let ahead = standing(Vec3::new(0.0, 0.0, 9.0));
        assert!(bounds(&jab, &ahead, false));
        let ticks = intersect(&jab, &ahead).expect("the jab lands");
        assert!((8..=11).contains(&ticks), "lands in its damage window ({ticks})");
        assert!(!bounds(&jab, &standing(Vec3::new(0.0, 0.0, 30.0)), false), "out of reach");
        assert!(!bounds(&jab, &standing(Vec3::new(0.0, 0.0, -9.0)), false), "behind");
        assert_eq!(intersect(&jab, &standing(Vec3::new(0.0, 0.0, -9.0))), Err(Miss::Behind));
    }

    #[test]
    fn a_back_kick_reaches_behind() {
        let Some(c) = konoko() else { return };
        let kick = Attacker { clip: clip(&c, "KONCOMkick_bk"), tick: 0, air: None, sphere: 3.0, quirks: Default::default() };
        let behind = standing(Vec3::new(0.0, 0.0, -10.0));
        assert!(bounds(&kick, &behind, false));
        assert!(intersect(&kick, &behind).is_ok());
        assert!(!bounds(&kick, &standing(Vec3::new(0.0, 0.0, 10.0)), false));
    }

    #[test]
    fn a_target_lying_low_is_below_a_jab() {
        let Some(c) = konoko() else { return };
        let jab = Attacker { clip: clip(&c, "KONCOMcomb_p"), tick: 0, air: None, sphere: 3.0, quirks: Default::default() };
        let mut lying = standing(Vec3::new(0.0, 0.0, 9.0));
        lying.height = (0.0, 4.0);
        assert!(!bounds(&jab, &lying, false), "the jab passes over it");
    }
}
