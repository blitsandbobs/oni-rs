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
pub struct Prim {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Index into [`Scene::textures`], or `None` for an untextured primitive.
    pub texture: Option<usize>,
    /// Alpha-blended (transparent) material.
    pub blend: bool,
}

/// A loaded scene: primitives + the set of textures they reference.
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

/// Reads an accessor's floats (component type 5126) as rows of `comp` values.
fn read_floats(json: &Value, bin: &[u8], acc_idx: usize) -> Vec<f32> {
    let acc = &json["accessors"][acc_idx];
    let bv = &json["bufferViews"][acc["bufferView"].as_u64().unwrap() as usize];
    let off = bv["byteOffset"].as_u64().unwrap_or(0) as usize
        + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
    let count = acc["count"].as_u64().unwrap() as usize;
    let comp = comp_count(acc["type"].as_str().unwrap());
    let mut out = Vec::with_capacity(count * comp);
    for i in 0..count * comp {
        let o = off + i * 4;
        out.push(f32::from_le_bytes([
            bin[o],
            bin[o + 1],
            bin[o + 2],
            bin[o + 3],
        ]));
    }
    out
}

/// Reads an index accessor (u32, component type 5125).
fn read_indices(json: &Value, bin: &[u8], acc_idx: usize) -> Vec<u32> {
    let acc = &json["accessors"][acc_idx];
    let bv = &json["bufferViews"][acc["bufferView"].as_u64().unwrap() as usize];
    let off = bv["byteOffset"].as_u64().unwrap_or(0) as usize
        + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
    let count = acc["count"].as_u64().unwrap() as usize;
    (0..count)
        .map(|i| {
            let o = off + i * 4;
            u32::from_le_bytes([bin[o], bin[o + 1], bin[o + 2], bin[o + 3]])
        })
        .collect()
}

/// Loads a `.glb` file into a [`Scene`].
pub fn load(path: &Path) -> std::io::Result<Scene> {
    let bytes = std::fs::read(path)?;
    let dir = path.parent().unwrap_or(Path::new("."));

    // glb: 12-byte header, then chunks [len u32][type u32][data].
    let json_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let json_start = 20;
    let json: Value = serde_json::from_slice(&bytes[json_start..json_start + json_len])
        .map_err(std::io::Error::other)?;
    let bin_start = json_start + json_len + 8; // skip BIN chunk header
    let bin = &bytes[bin_start..];

    let nodes = json["nodes"].as_array().cloned().unwrap_or_default();

    // Parent map (from each node's `children`).
    let mut parent = vec![None; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        if let Some(children) = n["children"].as_array() {
            for c in children {
                parent[c.as_u64().unwrap() as usize] = Some(i);
            }
        }
    }
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
    let mut material_texture = |mat_idx: usize| -> (Option<usize>, bool) {
        let mat = &json["materials"][mat_idx];
        let blend = mat["alphaMode"].as_str() == Some("BLEND");
        let Some(ti) = mat["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64() else {
            return (None, blend);
        };
        let src = json["textures"][ti as usize]["source"].as_u64().unwrap() as usize;
        let uri = json["images"][src]["uri"].as_str().unwrap_or("");
        let path = normalize(dir, uri);
        let key = path.to_string_lossy().into_owned();
        (
            Some(*tex_index.entry(key).or_insert_with(|| {
                tex_paths.push(path);
                tex_paths.len() - 1
            })),
            blend,
        )
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

    for (wm, mesh_idx, _node) in jobs {
        let normal_mat = Mat4::from_mat3(glam::Mat3::from_mat4(wm));
        for prim in json["meshes"][mesh_idx]["primitives"].as_array().unwrap() {
            let attrs = &prim["attributes"];
            let Some(pos_acc) = attrs["POSITION"].as_u64() else {
                continue;
            };
            let pos = read_floats(&json, bin, pos_acc as usize);
            let nrm = attrs["NORMAL"]
                .as_u64()
                .map(|a| read_floats(&json, bin, a as usize));
            let uv = attrs["TEXCOORD_0"]
                .as_u64()
                .map(|a| read_floats(&json, bin, a as usize));
            let col = attrs["COLOR_0"]
                .as_u64()
                .map(|a| read_floats(&json, bin, a as usize));
            let idx = read_indices(&json, bin, prim["indices"].as_u64().unwrap() as usize);
            let (texture, blend) = prim["material"]
                .as_u64()
                .map(|m| material_texture(m as usize))
                .unwrap_or((None, false));

            let n = pos.len() / 3;
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
                let color = col
                    .as_ref()
                    .map(|a| [a[v * 3], a[v * 3 + 1], a[v * 3 + 2]])
                    .unwrap_or([1.0, 1.0, 1.0]);
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
fn normalize(dir: &Path, uri: &str) -> PathBuf {
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
