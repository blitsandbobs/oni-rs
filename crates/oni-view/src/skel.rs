//! Rigid skeletal characters: load a character `model.glb` keeping its bone
//! hierarchy (parts stay in bone-local space), load a `TRAM` animation clip,
//! and pose the skeleton at a given time.
//!
//! Oni characters are *rigid* — each bone carries a whole mesh part, and
//! animation rotates bones, so posing is `world[bone] = world[parent] · T · R`
//! with `R` taken from the clip (or the model's rest pose when a bone has no
//! track). Clips bind to bones by name, so any clip retargets to any character
//! with the same bone names.

use std::path::{Path, PathBuf};

use glam::{Mat4, Quat, Vec3};

use crate::gltf::{self, Vertex};

/// One bone: its rest transform and parent.
pub struct Bone {
    pub name: String,
    pub translation: Vec3,
    pub rotation: Quat,
    pub parent: Option<usize>,
}

/// One drawable part, in **bone-local** space (posed via its bone's matrix).
pub struct Part {
    pub bone: usize,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub texture: Option<PathBuf>,
    pub blend: bool,
    pub additive: bool,
}

/// A loaded character: skeleton + drawable parts.
pub struct Character {
    pub bones: Vec<Bone>,
    pub parts: Vec<Part>,
}

/// One bone's rotation track (bound by name).
pub struct Channel {
    pub bone_name: String,
    pub times: Vec<f32>,
    pub rotations: Vec<Quat>,
}

/// An animation clip.
pub struct Clip {
    pub duration: f32,
    pub channels: Vec<Channel>,
}

impl Character {
    /// World matrix per bone, posed by `clip` at `time` (rest pose when `clip`
    /// is `None` or a bone has no track).
    pub fn pose(&self, clip: Option<&Clip>, time: f32) -> Vec<Mat4> {
        let n = self.bones.len();
        let mut world = vec![Mat4::IDENTITY; n];
        for i in 0..n {
            let b = &self.bones[i];
            let rot = clip
                .and_then(|c| c.channels.iter().find(|ch| ch.bone_name == b.name))
                .map(|ch| sample(ch, time))
                .unwrap_or(b.rotation);
            let local = Mat4::from_rotation_translation(rot, b.translation);
            world[i] = match b.parent {
                Some(p) => world[p] * local,
                None => local,
            };
        }
        world
    }
}

/// Samples a rotation channel at `time` (linear/slerp between keys).
fn sample(ch: &Channel, time: f32) -> Quat {
    if ch.times.is_empty() {
        return Quat::IDENTITY;
    }
    if time <= ch.times[0] {
        return ch.rotations[0];
    }
    let last = ch.times.len() - 1;
    if time >= ch.times[last] {
        return ch.rotations[last];
    }
    let i = ch.times.partition_point(|&t| t <= time).max(1) - 1;
    let (t0, t1) = (ch.times[i], ch.times[i + 1]);
    let f = if t1 > t0 {
        (time - t0) / (t1 - t0)
    } else {
        0.0
    };
    ch.rotations[i].slerp(ch.rotations[i + 1], f)
}

fn f(a: &[serde_json::Value], i: usize) -> f32 {
    a.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32
}

/// Loads a character `model.glb` (bones + bone-local parts).
pub fn load_character(path: &Path) -> std::io::Result<Character> {
    let bytes = std::fs::read(path)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let (json, bin) = gltf::glb_json_bin(&bytes);
    let nodes = json["nodes"].as_array().cloned().unwrap_or_default();

    // Parent links from `children`.
    let mut parent = vec![None; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        if let Some(children) = n["children"].as_array() {
            for c in children {
                parent[c.as_u64().unwrap() as usize] = Some(i);
            }
        }
    }

    let mut bones = Vec::with_capacity(nodes.len());
    let mut parts = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        let t = n["translation"]
            .as_array()
            .map(|a| Vec3::new(f(a, 0), f(a, 1), f(a, 2)))
            .unwrap_or(Vec3::ZERO);
        let r = n["rotation"]
            .as_array()
            .map(|a| Quat::from_xyzw(f(a, 0), f(a, 1), f(a, 2), f(a, 3)))
            .unwrap_or(Quat::IDENTITY);
        bones.push(Bone {
            name: n["name"].as_str().unwrap_or("").to_string(),
            translation: t,
            rotation: r,
            parent: parent[i],
        });

        if let Some(mesh) = n["mesh"].as_u64() {
            for prim in json["meshes"][mesh as usize]["primitives"]
                .as_array()
                .unwrap()
            {
                let at = &prim["attributes"];
                let Some(pos_acc) = at["POSITION"].as_u64() else {
                    continue;
                };
                let pos = gltf::read_floats(&json, bin, pos_acc as usize);
                let nrm = at["NORMAL"]
                    .as_u64()
                    .map(|a| gltf::read_floats(&json, bin, a as usize));
                let uv = at["TEXCOORD_0"]
                    .as_u64()
                    .map(|a| gltf::read_floats(&json, bin, a as usize));
                let idx =
                    gltf::read_indices(&json, bin, prim["indices"].as_u64().unwrap() as usize);
                let (texture, blend, additive) = prim["material"]
                    .as_u64()
                    .map(|m| material(&json, dir, m as usize))
                    .unwrap_or((None, false, false));

                let vcount = pos.len() / 3;
                let mut verts = Vec::with_capacity(vcount);
                for v in 0..vcount {
                    let normal = nrm
                        .as_ref()
                        .map(|a| [a[v * 3], a[v * 3 + 1], a[v * 3 + 2]])
                        .unwrap_or([0.0, 1.0, 0.0]);
                    let uvv = uv
                        .as_ref()
                        .map(|a| [a[v * 2], a[v * 2 + 1]])
                        .unwrap_or([0.0, 0.0]);
                    verts.push(Vertex {
                        pos: [pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]],
                        normal,
                        uv: uvv,
                        color: [1.0, 1.0, 1.0],
                    });
                }
                parts.push(Part {
                    bone: i,
                    vertices: verts,
                    indices: idx,
                    texture,
                    blend,
                    additive,
                });
            }
        }
    }
    Ok(Character { bones, parts })
}

/// Loads an animation `.glb` clip (rotation tracks, bound by bone name).
pub fn load_clip(path: &Path) -> std::io::Result<Clip> {
    let bytes = std::fs::read(path)?;
    let (json, bin) = gltf::glb_json_bin(&bytes);
    let nodes = json["nodes"].as_array().cloned().unwrap_or_default();
    let anim = &json["animations"][0];
    let samplers = anim["samplers"].as_array().cloned().unwrap_or_default();

    let mut channels = Vec::new();
    let mut duration = 0.0f32;
    for ch in anim["channels"].as_array().cloned().unwrap_or_default() {
        if ch["target"]["path"].as_str() != Some("rotation") {
            continue;
        }
        let node = ch["target"]["node"].as_u64().unwrap() as usize;
        let bone_name = nodes[node]["name"].as_str().unwrap_or("").to_string();
        let s = &samplers[ch["sampler"].as_u64().unwrap() as usize];
        let times = gltf::read_floats(&json, bin, s["input"].as_u64().unwrap() as usize);
        let rot = gltf::read_floats(&json, bin, s["output"].as_u64().unwrap() as usize);
        let rotations: Vec<Quat> = rot
            .chunks_exact(4)
            .map(|q| Quat::from_xyzw(q[0], q[1], q[2], q[3]))
            .collect();
        if let Some(&t) = times.last() {
            duration = duration.max(t);
        }
        channels.push(Channel {
            bone_name,
            times,
            rotations,
        });
    }
    Ok(Clip {
        duration: duration.max(0.001),
        channels,
    })
}

/// Resolves a material's texture path + blend/additive flags.
fn material(json: &serde_json::Value, dir: &Path, idx: usize) -> (Option<PathBuf>, bool, bool) {
    let m = &json["materials"][idx];
    let blend = m["alphaMode"].as_str() == Some("BLEND");
    let additive = m["extras"]["additive"].as_bool().unwrap_or(false);
    let Some(ti) = m["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64() else {
        return (None, blend, additive);
    };
    let src = json["textures"][ti as usize]["source"].as_u64().unwrap() as usize;
    let uri = json["images"][src]["uri"].as_str().unwrap_or("");
    (Some(gltf::normalize(dir, uri)), blend, additive)
}
