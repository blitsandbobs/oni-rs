//! Minimal glTF binary (`.glb`) loader for the viewer — just enough to draw the
//! meshes our own converter writes (POSITION / NORMAL / TEXCOORD_0 / COLOR_0,
//! per-node translation+rotation, a base-colour texture, and alpha mode).
//!
//! We parse the JSON chunk with `serde_json` and read vertex/index data straight
//! out of the BIN chunk. Node transforms are baked into vertex positions so the
//! renderer can treat the whole level as one flat list of primitives.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glam::{Mat4, Quat, Vec3};
use serde_json::Value;

/// A single vertex, matching the renderer's vertex buffer layout.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 3],
}

/// One drawable primitive (world-space, baked transform).
#[derive(Clone)]
pub struct Prim {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Index into [`Scene::textures`], or `None` for an untextured primitive.
    pub texture: Option<usize>,
    /// Alpha-blended (transparent) material.
    pub blend: bool,
    /// Additive-blended (glowing) material — from Oni texture flags (`extras`).
    pub additive: bool,
    /// The door (its object id) this is the closed leaf of — a node named
    /// `door_0x<id>`, or under one.
    pub door: Option<u32>,
    /// Which of the door's leaves (double doors have two).
    pub leaf: u8,
    /// Drawn (a node named `…__invisible` only collides).
    pub visible: bool,
    /// Characters collide with it (not a static-world node named
    /// `…__nocollide`; placed objects always do).
    pub collide: bool,
    /// The reference scripts show and hide it by (`…__ref<N>…`).
    pub env_ref: Option<u32>,
    /// A breakable glass pane, by its quad (`…__glass<N>`).
    pub glass: Option<u32>,
    /// Drawn from its front only (its material says `doubleSided: false`: a
    /// level quad the original culls from behind).
    pub one_sided: bool,
}

/// A loaded scene: primitives + the set of textures they reference.
#[derive(Clone)]
pub struct Scene {
    pub prims: Vec<Prim>,
    pub textures: Vec<PathBuf>,
    pub min: Vec3,
    pub max: Vec3,
}

impl Scene {
    /// Centre of the scene's bounding box.
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    /// Largest bounding-box extent.
    pub fn extent(&self) -> f32 {
        (self.max - self.min).max_element().max(1.0)
    }
}

fn comp_count(ty: &str) -> usize {
    match ty {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        _ => 0,
    }
}

/// A malformed file's error.
pub(crate) fn bad(what: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, format!("glb: {what}"))
}

/// A JSON index (`what` names it in the error).
pub(crate) fn index(v: &Value, what: &str) -> std::io::Result<usize> {
    v.as_u64().map(|i| i as usize).ok_or_else(|| bad(what))
}

/// Splits a `.glb` into its JSON chunk and BIN chunk data.
pub(crate) fn glb_json_bin(bytes: &[u8]) -> std::io::Result<(Value, &[u8])> {
    let len = bytes.get(12..16).ok_or_else(|| bad("truncated header"))?;
    let json_len = u32::from_le_bytes([len[0], len[1], len[2], len[3]]) as usize;
    let chunk = bytes
        .get(20..20 + json_len)
        .ok_or_else(|| bad("truncated JSON chunk"))?;
    let json: Value = serde_json::from_slice(chunk).map_err(|e| bad(&format!("JSON: {e}")))?;
    let bin = bytes.get(20 + json_len + 8..).unwrap_or(&[]);
    Ok((json, bin))
}

/// An accessor's data: where it starts in the BIN chunk, and how many
/// elements of how many components.
fn accessor(json: &Value, acc_idx: usize) -> std::io::Result<(usize, usize, usize)> {
    let acc = &json["accessors"][acc_idx];
    let bv = &json["bufferViews"][index(&acc["bufferView"], "accessor's buffer view")?];
    let off = bv["byteOffset"].as_u64().unwrap_or(0) as usize
        + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
    let count = index(&acc["count"], "accessor's count")?;
    let comp = comp_count(acc["type"].as_str().unwrap_or("SCALAR"));
    Ok((off, count, comp))
}

/// `n` little-endian 4-byte words from `off` (none past the chunk's end).
fn words(bin: &[u8], off: usize, n: usize) -> std::io::Result<impl Iterator<Item = [u8; 4]> + '_> {
    let data = bin
        .get(off..off + n * 4)
        .ok_or_else(|| bad("accessor past the BIN chunk"))?;
    Ok(data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| [w[0], w[1], w[2], w[3]]))
}

/// Reads an accessor's floats (component type 5126) as rows of `comp` values.
pub(crate) fn read_floats(json: &Value, bin: &[u8], acc_idx: usize) -> std::io::Result<Vec<f32>> {
    let (off, count, comp) = accessor(json, acc_idx)?;
    Ok(words(bin, off, count * comp)?
        .map(f32::from_le_bytes)
        .collect())
}

/// Reads an index accessor (u32, component type 5125).
pub(crate) fn read_indices(json: &Value, bin: &[u8], acc_idx: usize) -> std::io::Result<Vec<u32>> {
    let (off, count, _) = accessor(json, acc_idx)?;
    Ok(words(bin, off, count)?.map(u32::from_le_bytes).collect())
}

/// Checks a primitive's attributes and indices against its `n` vertices
/// (`per`: each attribute's components a vertex — 3 for normals and
/// colours, 2 for UVs): a short attribute or an index past the vertices is
/// an error, not a panic later.
pub(crate) fn check_primitive(
    n: usize,
    attrs: &[(Option<&Vec<f32>>, usize)],
    idx: &[u32],
) -> std::io::Result<()> {
    for &(a, per) in attrs {
        if a.is_some_and(|a| a.len() < n * per) {
            return Err(bad("attribute shorter than the positions"));
        }
    }
    if idx.iter().any(|&i| i as usize >= n) {
        return Err(bad("index past the vertices"));
    }
    Ok(())
}

/// Each node's parent (from the nodes' `children`).
pub(crate) fn parents(nodes: &[Value]) -> std::io::Result<Vec<Option<usize>>> {
    let mut parent = vec![None; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        for c in n["children"].as_array().into_iter().flatten() {
            *parent
                .get_mut(index(c, "child")?)
                .ok_or_else(|| bad("child past the nodes"))? = Some(i);
        }
    }
    Ok(parent)
}

/// A material as drawn: its base colour texture's image (its `uri`), and
/// whether it blends, adds, lights itself.
pub(crate) struct Material {
    pub uri: Option<String>,
    pub blend: bool,
    pub additive: bool,
    pub self_illum: bool,
}

pub(crate) fn material(json: &Value, idx: usize) -> std::io::Result<Material> {
    let m = &json["materials"][idx];
    let uri = match m["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64() {
        None => None,
        Some(ti) => {
            let src = index(&json["textures"][ti as usize]["source"], "texture's image")?;
            Some(
                json["images"][src]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            )
        }
    };
    Ok(Material {
        uri,
        blend: m["alphaMode"].as_str() == Some("BLEND"),
        additive: m["extras"]["additive"].as_bool().unwrap_or(false),
        self_illum: m["extras"]["selfIllum"].as_bool().unwrap_or(false),
    })
}

/// Loads a `.glb` file into a [`Scene`].
pub fn load(path: &Path) -> std::io::Result<Scene> {
    let bytes = std::fs::read(path)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let (json, bin) = glb_json_bin(&bytes)?;

    let nodes = json["nodes"].as_array().cloned().unwrap_or_default();
    let parent = parents(&nodes)?;
    let local = |n: &Value| -> Mat4 {
        let t = n["translation"]
            .as_array()
            .map(|a| Vec3::new(f(a, 0), f(a, 1), f(a, 2)))
            .unwrap_or(Vec3::ZERO);
        let r = n["rotation"]
            .as_array()
            .map(|a| Quat::from_xyzw(f(a, 0), f(a, 1), f(a, 2), f(a, 3)))
            .unwrap_or(Quat::IDENTITY);
        Mat4::from_rotation_translation(r, t)
    };
    let world = |i: usize| -> Mat4 {
        let mut m = Mat4::IDENTITY;
        let mut chain = Vec::new();
        let mut cur = Some(i);
        while let Some(j) = cur {
            chain.push(j);
            cur = parent[j];
        }
        for &j in chain.iter().rev() {
            m *= local(&nodes[j]);
        }
        m
    };

    // Texture path per material (deduplicated).
    let mut tex_paths: Vec<PathBuf> = Vec::new();
    let mut tex_index: BTreeMap<String, usize> = BTreeMap::new();
    // Returns (texture index, blend, additive, self_illuminated).
    // (Sidedness is read on its own, below.)
    let mut material_texture =
        |mat_idx: usize| -> std::io::Result<(Option<usize>, bool, bool, bool)> {
            let m = material(&json, mat_idx)?;
            let idx = m.uri.map(|uri| {
                let path = normalize(dir, &uri);
                let key = path.to_string_lossy().into_owned();
                *tex_index.entry(key).or_insert_with(|| {
                    tex_paths.push(path);
                    tex_paths.len() - 1
                })
            });
            Ok((idx, m.blend, m.additive, m.self_illum))
        };

    let mut prims = Vec::new();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);

    // `material_texture` borrows tex_paths/tex_index mutably, so collect per node.
    let mut jobs: Vec<(Mat4, usize, usize)> = Vec::new(); // (world, mesh, node)
    for (i, n) in nodes.iter().enumerate() {
        if let Some(mesh) = n["mesh"].as_u64() {
            jobs.push((world(i), mesh as usize, i));
        }
    }

    // The door a node belongs to (itself or an ancestor named `door_0x…`).
    let door_of = |i: usize| -> Option<u32> {
        let mut cur = Some(i);
        while let Some(j) = cur {
            let name = nodes[j]["name"].as_str().unwrap_or("");
            if let Some(hex) = name.strip_prefix("door_0x") {
                let digits: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                return u32::from_str_radix(&digits, 16).ok();
            }
            cur = parent[j];
        }
        None
    };
    for (wm, mesh_idx, node) in jobs {
        let door = door_of(node);
        let name = nodes[node]["name"].as_str().unwrap_or("");
        // (Placed objects — under a pivot node — are flagged not to collide
        // as level geometry because the object itself collides: they do.)
        let object = parent[node].is_some();
        let (visible, collide) = (
            !name.ends_with("__invisible"),
            object || !name.ends_with("__nocollide"),
        );
        let number_after = |tag: &str| -> Option<u32> {
            name.split(tag).nth(1).and_then(|r| {
                r.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .ok()
            })
        };
        let (env_ref, glass) = (number_after("__ref"), number_after("__glass"));
        let normal_mat = Mat4::from_mat3(glam::Mat3::from_mat4(wm));
        let prims_of = json["meshes"][mesh_idx]["primitives"].as_array();
        for prim in prims_of.into_iter().flatten() {
            let attrs = &prim["attributes"];
            let Some(pos_acc) = attrs["POSITION"].as_u64() else {
                continue;
            };
            let floats = |a: u64| read_floats(&json, bin, a as usize);
            let pos = floats(pos_acc)?;
            let nrm = attrs["NORMAL"].as_u64().map(floats).transpose()?;
            let uv = attrs["TEXCOORD_0"].as_u64().map(floats).transpose()?;
            let col = attrs["COLOR_0"].as_u64().map(floats).transpose()?;
            let idx = read_indices(&json, bin, index(&prim["indices"], "primitive's indices")?)?;
            let (texture, blend, additive, self_illum) = match prim["material"].as_u64() {
                Some(m) => material_texture(m as usize)?,
                None => (None, false, false, false),
            };
            let one_sided = prim["material"].as_u64().is_some_and(|m| {
                json["materials"][m as usize]["doubleSided"].as_bool() == Some(false)
            });
            // Self-lit / additive surfaces ignore baked lighting (full bright).
            let full_bright = additive || self_illum;

            let n = pos.len() / 3;
            check_primitive(
                n,
                &[(nrm.as_ref(), 3), (uv.as_ref(), 2), (col.as_ref(), 3)],
                &idx,
            )?;
            let mut verts = Vec::with_capacity(n);
            for v in 0..n {
                let p = wm.transform_point3(Vec3::new(pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]));
                min = min.min(p);
                max = max.max(p);
                let normal = match &nrm {
                    Some(a) => normal_mat
                        .transform_vector3(Vec3::new(a[v * 3], a[v * 3 + 1], a[v * 3 + 2]))
                        .normalize_or_zero(),
                    None => Vec3::Y,
                };
                let uvv = uv
                    .as_ref()
                    .map(|a| [a[v * 2], a[v * 2 + 1]])
                    .unwrap_or([0.0, 0.0]);
                let color = if full_bright {
                    [1.0, 1.0, 1.0]
                } else {
                    col.as_ref()
                        .map(|a| [a[v * 3], a[v * 3 + 1], a[v * 3 + 2]])
                        .unwrap_or([1.0, 1.0, 1.0])
                };
                verts.push(Vertex {
                    pos: p.into(),
                    normal: normal.into(),
                    uv: uvv,
                    color,
                });
            }
            prims.push(Prim {
                vertices: verts,
                indices: idx,
                texture,
                blend,
                additive,
                door,
                leaf: 0,
                visible,
                collide,
                env_ref,
                glass,
                one_sided,
            });
        }
    }

    Ok(Scene {
        prims,
        textures: tex_paths,
        min,
        max,
    })
}

fn f(a: &[Value], i: usize) -> f32 {
    a.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32
}

/// Resolves a (possibly `../`-prefixed) glTF URI against the glb's directory.
pub(crate) fn normalize(dir: &Path, uri: &str) -> PathBuf {
    let mut p = dir.to_path_buf();
    for seg in uri.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                p.pop();
            }
            s => p.push(s),
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small `.glb`: one node with a triangle (positions, normals, UVs,
    /// indices — `normals` of them, and the index `last`) and a one-key
    /// rotation clip on it.
    fn glb(normals: usize, last: u32) -> Vec<u8> {
        let mut bin: Vec<u8> = Vec::new();
        let mut put = |v: &[f32]| v.iter().for_each(|x| bin.extend(x.to_le_bytes()));
        put(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]); // positions @0
        put(&[0.0; 9]); // normals @36
        put(&[0.0; 6]); // uvs @72
        put(&[0.0]); // time @96
        put(&[0.0, 0.0, 0.0, 1.0]); // rotation @100
        for i in [0u32, 1, last] {
            bin.extend(i.to_le_bytes()); // indices @116
        }
        let view = |off: usize, len: usize| serde_json::json!({"buffer": 0, "byteOffset": off, "byteLength": len});
        let acc = |v: usize, n: usize, ty: &str| serde_json::json!({"bufferView": v, "count": n, "type": ty});
        let json = serde_json::json!({
            "nodes": [{"name": "pelvis", "mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}, "indices": 3}]}],
            "accessors": [acc(0, 3, "VEC3"), acc(1, normals, "VEC3"), acc(2, 3, "VEC2"), acc(5, 3, "SCALAR"),
                          acc(3, 1, "SCALAR"), acc(4, 1, "VEC4")],
            "bufferViews": [view(0, 36), view(36, 36), view(72, 24), view(96, 4), view(100, 16), view(116, 12)],
            "animations": [{"samplers": [{"input": 4, "output": 5}],
                            "channels": [{"sampler": 0, "target": {"node": 0, "path": "rotation"}}]}],
        });
        let mut j = serde_json::to_vec(&json).unwrap();
        while !j.len().is_multiple_of(4) {
            j.push(b' ');
        }
        let mut out = Vec::new();
        out.extend(b"glTF");
        out.extend(2u32.to_le_bytes());
        out.extend(((12 + 8 + j.len() + 8 + bin.len()) as u32).to_le_bytes());
        out.extend((j.len() as u32).to_le_bytes());
        out.extend(0x4E4F_534Au32.to_le_bytes());
        out.extend(&j);
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(0x004E_4942u32.to_le_bytes());
        out.extend(&bin);
        out
    }

    fn write(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("oni_glb_{}_{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.glb");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// A whole file loads; one cut short anywhere, with an attribute
    /// shorter than its positions, or an index past its vertices is an
    /// error — through every loader, never a panic.
    #[test]
    fn a_malformed_glb_is_an_error() {
        let good = glb(3, 2);
        let path = write("good", &good);
        assert!(load(&path).is_ok(), "a scene");
        assert!(crate::skel::load_character(&path).is_ok(), "a character");
        assert!(crate::skel::load_clip(&path).is_ok(), "a clip");
        for cut in [0, 10, 19, 40, good.len() / 2, good.len() - 4] {
            let path = write("cut", &good[..cut]);
            assert!(load(&path).is_err(), "scene cut at {cut}");
            assert!(
                crate::skel::load_character(&path).is_err(),
                "character cut at {cut}"
            );
            // (The last cut loses only index bytes, which a clip never reads.)
            if cut < good.len() - 12 {
                assert!(crate::skel::load_clip(&path).is_err(), "clip cut at {cut}");
            }
        }
        for (what, bytes) in [("short normals", glb(2, 2)), ("index past", glb(3, 3))] {
            let path = write("bad", &bytes);
            assert!(load(&path).is_err(), "scene: {what}");
            assert!(
                crate::skel::load_character(&path).is_err(),
                "character: {what}"
            );
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
