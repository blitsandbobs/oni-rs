//! A minimal glTF 2.0 binary (`.glb`) writer for a single [`crate::Mesh`].
//!
//! Hand-built (no serde/gltf dependency) because the structure is fixed: one
//! buffer, up to four accessors (indices, POSITION, NORMAL, TEXCOORD_0), one
//! mesh/primitive, and an optional material referencing an external texture.

use crate::Mesh;

const COMP_U32: u32 = 5125;
const COMP_F32: u32 = 5126;
const TARGET_ARRAY_BUFFER: u32 = 34963; // ELEMENT_ARRAY_BUFFER for indices
const TARGET_VERTEX_BUFFER: u32 = 34962; // ARRAY_BUFFER for attributes

/// Serializes `mesh` to a self-contained `.glb`. When `texture_uri` is given,
/// the material's base-colour texture points at it (a relative path to a PNG).
pub fn to_glb(mesh: &Mesh, texture_uri: Option<&str>) -> Vec<u8> {
    // --- assemble the binary buffer ---
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new(); // (byte_offset, byte_len, target)
    let mut accessors = String::new();
    let mut acc_count = 0u32;

    // indices
    let idx_acc = acc_count;
    {
        let off = bin.len();
        for &i in &mesh.indices {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        views.push((off, bin.len() - off, TARGET_ARRAY_BUFFER));
        push_accessor(
            &mut accessors,
            views.len() - 1,
            COMP_U32,
            mesh.indices.len(),
            "SCALAR",
            None,
        );
        acc_count += 1;
    }

    // positions (with min/max)
    let pos_acc = acc_count;
    {
        let off = bin.len();
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &mesh.positions {
            for (k, &val) in p.iter().enumerate() {
                min[k] = min[k].min(val);
                max[k] = max[k].max(val);
                bin.extend_from_slice(&val.to_le_bytes());
            }
        }
        views.push((off, bin.len() - off, TARGET_VERTEX_BUFFER));
        let mm = format!(
            "\"min\":[{},{},{}],\"max\":[{},{},{}]",
            f(min[0]),
            f(min[1]),
            f(min[2]),
            f(max[0]),
            f(max[1]),
            f(max[2])
        );
        push_accessor(
            &mut accessors,
            views.len() - 1,
            COMP_F32,
            mesh.positions.len(),
            "VEC3",
            Some(&mm),
        );
        acc_count += 1;
    }

    // normals
    let normal_acc = if !mesh.normals.is_empty() {
        let off = bin.len();
        for n in &mesh.normals {
            for &val in n.iter() {
                bin.extend_from_slice(&val.to_le_bytes());
            }
        }
        views.push((off, bin.len() - off, TARGET_VERTEX_BUFFER));
        push_accessor(
            &mut accessors,
            views.len() - 1,
            COMP_F32,
            mesh.normals.len(),
            "VEC3",
            None,
        );
        let a = acc_count;
        acc_count += 1;
        Some(a)
    } else {
        None
    };

    // texcoords
    let uv_acc = if !mesh.texcoords.is_empty() {
        let off = bin.len();
        for t in &mesh.texcoords {
            bin.extend_from_slice(&t[0].to_le_bytes());
            bin.extend_from_slice(&t[1].to_le_bytes());
        }
        views.push((off, bin.len() - off, TARGET_VERTEX_BUFFER));
        push_accessor(
            &mut accessors,
            views.len() - 1,
            COMP_F32,
            mesh.texcoords.len(),
            "VEC2",
            None,
        );
        let a = acc_count;
        acc_count += 1;
        Some(a)
    } else {
        None
    };
    let _ = acc_count; // last accessor index consumed above

    // --- bufferViews JSON ---
    let mut bv = String::new();
    for (i, (off, len, target)) in views.iter().enumerate() {
        if i > 0 {
            bv.push(',');
        }
        bv.push_str(&format!(
            "{{\"buffer\":0,\"byteOffset\":{off},\"byteLength\":{len},\"target\":{target}}}"
        ));
    }

    // --- primitive attributes ---
    let mut attrs = format!("\"POSITION\":{pos_acc}");
    if let Some(a) = normal_acc {
        attrs.push_str(&format!(",\"NORMAL\":{a}"));
    }
    if let Some(a) = uv_acc {
        attrs.push_str(&format!(",\"TEXCOORD_0\":{a}"));
    }

    // --- optional texture/material ---
    let (images, textures, samplers, materials, mat_ref) = match texture_uri {
        Some(uri) => (
            format!("\"images\":[{{\"uri\":{}}}],", json_str(uri)),
            "\"textures\":[{\"source\":0,\"sampler\":0}],".to_string(),
            "\"samplers\":[{}],".to_string(),
            "\"materials\":[{\"pbrMetallicRoughness\":{\"baseColorTexture\":{\"index\":0},\"metallicFactor\":0,\"roughnessFactor\":1},\"doubleSided\":true}],".to_string(),
            ",\"material\":0",
        ),
        None => (
            String::new(),
            String::new(),
            String::new(),
            "\"materials\":[{\"doubleSided\":true}],".to_string(),
            ",\"material\":0",
        ),
    };

    let json = format!(
        "{{\
\"asset\":{{\"version\":\"2.0\",\"generator\":\"oni-geom\"}},\
\"scene\":0,\
\"scenes\":[{{\"nodes\":[0]}}],\
\"nodes\":[{{\"mesh\":0}}],\
\"meshes\":[{{\"primitives\":[{{\"attributes\":{{{attrs}}},\"indices\":{idx_acc},\"mode\":4{mat_ref}}}]}}],\
{images}{textures}{samplers}{materials}\
\"accessors\":[{accessors}],\
\"bufferViews\":[{bv}],\
\"buffers\":[{{\"byteLength\":{}}}]\
}}",
        bin.len()
    );

    assemble_glb(&json, &bin)
}

fn push_accessor(
    s: &mut String,
    buffer_view: usize,
    component_type: u32,
    count: usize,
    ty: &str,
    extra: Option<&str>,
) {
    if !s.is_empty() {
        s.push(',');
    }
    s.push_str(&format!(
        "{{\"bufferView\":{buffer_view},\"componentType\":{component_type},\"count\":{count},\"type\":\"{ty}\""
    ));
    if let Some(e) = extra {
        s.push(',');
        s.push_str(e);
    }
    s.push('}');
}

/// Formats a float for JSON output.
fn f(v: f32) -> String {
    format!("{v}")
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn assemble_glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json_bytes = json.as_bytes().to_vec();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let mut bin_bytes = bin.to_vec();
    while !bin_bytes.len().is_multiple_of(4) {
        bin_bytes.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin_bytes.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&0x4654_6C67u32.to_le_bytes()); // "glTF"
    out.extend_from_slice(&2u32.to_le_bytes()); // version
    out.extend_from_slice(&(total as u32).to_le_bytes());
    // JSON chunk
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes()); // "JSON"
    out.extend_from_slice(&json_bytes);
    // BIN chunk
    out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes()); // "BIN\0"
    out.extend_from_slice(&bin_bytes);
    out
}

// ===========================================================================
// Multi-node scene writer (for skeletal characters: one node per body part).
// ===========================================================================

/// One node in a glTF scene: a transform, an optional mesh, and a parent.
pub struct SceneNode<'a> {
    /// Node name.
    pub name: String,
    /// Translation relative to the parent node.
    pub translation: [f32; 3],
    /// Rotation quaternion `[x,y,z,w]` (glTF composes `local = T·R`, matching
    /// the engine's pose math).
    pub rotation: [f32; 4],
    /// Parent node index, or `None` for a root.
    pub parent: Option<usize>,
    /// The mesh drawn at this node, or `None` for a transform-only node (e.g. a
    /// placed object's pivot parent).
    pub mesh: Option<&'a Mesh>,
    /// Relative URI of this node's base texture, if any.
    pub texture_uri: Option<String>,
    /// Whether this node's material is alpha-blended (transparent).
    pub blend: bool,
    /// Additive-blended (glowing) material — carried in material `extras`.
    pub additive: bool,
    /// Self-illuminated (full-bright) material — carried in material `extras`.
    pub self_illum: bool,
    /// Drawn from its front only (the material's `doubleSided` false): a
    /// level quad the original culls from behind.
    pub one_sided: bool,
}

/// Serializes a node hierarchy (each node carrying a mesh) to a `.glb`.
pub fn scene_to_glb(nodes: &[SceneNode]) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<(usize, usize, u32)> = Vec::new();
    let mut accessors = String::new();
    let mut acc_count = 0u32;
    let mut meshes = String::new();

    // Deduplicate textures/materials by (URI, blend, additive, self_illum).
    let mut tex_uris: Vec<String> = Vec::new();
    let mut mat_for_uri: std::collections::BTreeMap<(String, bool, bool, bool, bool), usize> =
        std::collections::BTreeMap::new();
    let mut tex_for_uri: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut materials: Vec<String> = Vec::new();
    let default_mat = {
        materials.push("{\"doubleSided\":true}".to_string());
        0usize
    };

    // Track which glTF mesh index (if any) each node draws.
    let mut mesh_for_node: Vec<Option<usize>> = Vec::with_capacity(nodes.len());
    let mut mesh_count = 0usize;
    for n in nodes {
        let Some(mesh) = n.mesh else {
            mesh_for_node.push(None);
            continue;
        };
        // Append this mesh's accessors.
        let (attrs, idx_acc) =
            append_mesh(&mut bin, &mut views, &mut accessors, &mut acc_count, mesh);
        // Resolve material, deduplicated by (texture, blend, additive, selfIllum).
        let mat = match &n.texture_uri {
            Some(uri) => *mat_for_uri
                .entry((uri.clone(), n.blend, n.additive, n.self_illum, n.one_sided))
                .or_insert_with(|| {
                    let img = *tex_for_uri.entry(uri.clone()).or_insert_with(|| {
                        let i = tex_uris.len();
                        tex_uris.push(uri.clone());
                        i
                    });
                    // Additive implies transparency.
                    let alpha = if n.blend || n.additive {
                        ",\"alphaMode\":\"BLEND\""
                    } else {
                        ""
                    };
                    // Oni-specific render flags ride in `extras`.
                    let extras = if n.additive || n.self_illum {
                        format!(
                            ",\"extras\":{{\"additive\":{},\"selfIllum\":{}}}",
                            n.additive, n.self_illum
                        )
                    } else {
                        String::new()
                    };
                    let m = materials.len();
                    materials.push(format!(
                        "{{\"pbrMetallicRoughness\":{{\"baseColorTexture\":{{\"index\":{img}}},\"metallicFactor\":0,\"roughnessFactor\":1}},\"doubleSided\":{}{alpha}{extras}}}",
                        !n.one_sided
                    ));
                    m
                }),
            None => default_mat,
        };
        if !meshes.is_empty() {
            meshes.push(',');
        }
        meshes.push_str(&format!(
            "{{\"primitives\":[{{\"attributes\":{{{attrs}}},\"indices\":{idx_acc},\"mode\":4,\"material\":{mat}}}]}}"
        ));
        mesh_for_node.push(Some(mesh_count));
        mesh_count += 1;
    }

    // Nodes JSON with children + translation + mesh.
    let mut node_json = String::new();
    for (i, n) in nodes.iter().enumerate() {
        let children: Vec<String> = nodes
            .iter()
            .enumerate()
            .filter(|(j, m)| *j != i && m.parent == Some(i))
            .map(|(j, _)| j.to_string())
            .collect();
        if i > 0 {
            node_json.push(',');
        }
        let [tx, ty, tz] = n.translation;
        let mesh_ref = match mesh_for_node[i] {
            Some(m) => format!(",\"mesh\":{m}"),
            None => String::new(),
        };
        node_json.push_str(&format!(
            "{{\"name\":{}{mesh_ref},\"translation\":[{},{},{}]",
            json_str(&n.name),
            f(tx),
            f(ty),
            f(tz)
        ));
        let [rx, ry, rz, rw] = n.rotation;
        if (rx, ry, rz, rw) != (0.0, 0.0, 0.0, 1.0) {
            node_json.push_str(&format!(
                ",\"rotation\":[{},{},{},{}]",
                f(rx),
                f(ry),
                f(rz),
                f(rw)
            ));
        }
        if !children.is_empty() {
            node_json.push_str(&format!(",\"children\":[{}]", children.join(",")));
        }
        if let Some([cx, cy, cz, r]) = n.mesh.and_then(|m| m.bound_sphere) {
            node_json.push_str(&format!(
                ",\"extras\":{{\"boundSphere\":[{},{},{},{}]}}",
                f(cx),
                f(cy),
                f(cz),
                f(r)
            ));
        }
        node_json.push('}');
    }
    let roots: Vec<String> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.parent.is_none())
        .map(|(i, _)| i.to_string())
        .collect();

    // bufferViews
    let mut bv = String::new();
    for (i, (off, len, target)) in views.iter().enumerate() {
        if i > 0 {
            bv.push(',');
        }
        bv.push_str(&format!(
            "{{\"buffer\":0,\"byteOffset\":{off},\"byteLength\":{len},\"target\":{target}}}"
        ));
    }

    let images = if tex_uris.is_empty() {
        String::new()
    } else {
        let imgs: Vec<String> = tex_uris
            .iter()
            .map(|u| format!("{{\"uri\":{}}}", json_str(u)))
            .collect();
        let texs: Vec<String> = (0..tex_uris.len())
            .map(|i| format!("{{\"source\":{i},\"sampler\":0}}"))
            .collect();
        format!(
            "\"images\":[{}],\"textures\":[{}],\"samplers\":[{{}}],",
            imgs.join(","),
            texs.join(",")
        )
    };

    let json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"oni-geom\"}},\
\"scene\":0,\"scenes\":[{{\"nodes\":[{}]}}],\
\"nodes\":[{node_json}],\
\"meshes\":[{meshes}],\
{images}\
\"materials\":[{}],\
\"accessors\":[{accessors}],\
\"bufferViews\":[{bv}],\
\"buffers\":[{{\"byteLength\":{}}}]}}",
        roots.join(","),
        materials.join(","),
        bin.len()
    );

    assemble_glb(&json, &bin)
}

/// Appends a mesh's vertex/index data to `bin` and its accessors, returning the
/// primitive's attribute JSON and the indices accessor index.
fn append_mesh(
    bin: &mut Vec<u8>,
    views: &mut Vec<(usize, usize, u32)>,
    accessors: &mut String,
    acc_count: &mut u32,
    mesh: &Mesh,
) -> (String, u32) {
    // indices
    let off = bin.len();
    for &i in &mesh.indices {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    views.push((off, bin.len() - off, TARGET_ARRAY_BUFFER));
    push_accessor(
        accessors,
        views.len() - 1,
        COMP_U32,
        mesh.indices.len(),
        "SCALAR",
        None,
    );
    let idx_acc = *acc_count;
    *acc_count += 1;

    // positions
    let off = bin.len();
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for (k, &v) in p.iter().enumerate() {
            min[k] = min[k].min(v);
            max[k] = max[k].max(v);
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    views.push((off, bin.len() - off, TARGET_VERTEX_BUFFER));
    let mm = format!(
        "\"min\":[{},{},{}],\"max\":[{},{},{}]",
        f(min[0]),
        f(min[1]),
        f(min[2]),
        f(max[0]),
        f(max[1]),
        f(max[2])
    );
    push_accessor(
        accessors,
        views.len() - 1,
        COMP_F32,
        mesh.positions.len(),
        "VEC3",
        Some(&mm),
    );
    let pos_acc = *acc_count;
    *acc_count += 1;

    let mut attrs = format!("\"POSITION\":{pos_acc}");

    if !mesh.normals.is_empty() {
        let off = bin.len();
        for n in &mesh.normals {
            for &v in n.iter() {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        views.push((off, bin.len() - off, TARGET_VERTEX_BUFFER));
        push_accessor(
            accessors,
            views.len() - 1,
            COMP_F32,
            mesh.normals.len(),
            "VEC3",
            None,
        );
        attrs.push_str(&format!(",\"NORMAL\":{}", *acc_count));
        *acc_count += 1;
    }
    if !mesh.texcoords.is_empty() {
        let off = bin.len();
        for t in &mesh.texcoords {
            bin.extend_from_slice(&t[0].to_le_bytes());
            bin.extend_from_slice(&t[1].to_le_bytes());
        }
        views.push((off, bin.len() - off, TARGET_VERTEX_BUFFER));
        push_accessor(
            accessors,
            views.len() - 1,
            COMP_F32,
            mesh.texcoords.len(),
            "VEC2",
            None,
        );
        attrs.push_str(&format!(",\"TEXCOORD_0\":{}", *acc_count));
        *acc_count += 1;
    }
    if !mesh.colors.is_empty() {
        let off = bin.len();
        for c in &mesh.colors {
            for &v in c.iter() {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        views.push((off, bin.len() - off, TARGET_VERTEX_BUFFER));
        push_accessor(
            accessors,
            views.len() - 1,
            COMP_F32,
            mesh.colors.len(),
            "VEC3",
            None,
        );
        attrs.push_str(&format!(",\"COLOR_0\":{}", *acc_count));
        *acc_count += 1;
    }

    (attrs, idx_acc)
}

// ===========================================================================
// Animation-clip writer: a named-bone skeleton (no mesh) + one animation.
// ===========================================================================

/// A skeleton joint for an animation rig (no mesh).
pub struct Joint {
    /// Bone name.
    pub name: String,
    /// Translation relative to the parent.
    pub translation: [f32; 3],
    /// Parent joint index, or `None` for a root.
    pub parent: Option<usize>,
}

/// One bone's rotation track (times in seconds, one quaternion per time).
pub struct AnimChannel<'a> {
    /// Target joint index.
    pub node: usize,
    /// Keyframe times (seconds).
    pub times: &'a [f32],
    /// Quaternion `[x,y,z,w]` per keyframe.
    pub rotations: &'a [[f32; 4]],
}

/// Serializes a bone skeleton plus one rotation animation to a `.glb`. The
/// skeleton has named joints (no mesh), so the clip retargets to any character
/// with the same bone names.
pub fn animation_to_glb(
    joints: &[Joint],
    anim_name: &str,
    channels: &[AnimChannel],
    extras_json: &str,
) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<(usize, usize)> = Vec::new();
    let mut accessors = String::new();
    let mut acc_count = 0u32;

    let mut samplers = String::new();
    let mut chans_json = String::new();

    for ch in channels {
        // input accessor: times (needs min/max)
        let off = bin.len();
        let (mut tmin, mut tmax) = (f32::INFINITY, f32::NEG_INFINITY);
        for &t in ch.times {
            tmin = tmin.min(t);
            tmax = tmax.max(t);
            bin.extend_from_slice(&t.to_le_bytes());
        }
        views.push((off, bin.len() - off));
        let mm = format!("\"min\":[{}],\"max\":[{}]", f(tmin), f(tmax));
        push_accessor(
            &mut accessors,
            views.len() - 1,
            COMP_F32,
            ch.times.len(),
            "SCALAR",
            Some(&mm),
        );
        let input_acc = acc_count;
        acc_count += 1;

        // output accessor: quaternions
        let off = bin.len();
        for q in ch.rotations {
            for &v in q.iter() {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        views.push((off, bin.len() - off));
        push_accessor(
            &mut accessors,
            views.len() - 1,
            COMP_F32,
            ch.rotations.len(),
            "VEC4",
            None,
        );
        let output_acc = acc_count;
        acc_count += 1;

        let si = samplers.matches("\"input\"").count();
        if !samplers.is_empty() {
            samplers.push(',');
        }
        samplers.push_str(&format!(
            "{{\"input\":{input_acc},\"output\":{output_acc},\"interpolation\":\"LINEAR\"}}"
        ));
        if !chans_json.is_empty() {
            chans_json.push(',');
        }
        chans_json.push_str(&format!(
            "{{\"sampler\":{si},\"target\":{{\"node\":{},\"path\":\"rotation\"}}}}",
            ch.node
        ));
    }

    // nodes (skeleton, no mesh)
    let mut node_json = String::new();
    for (i, j) in joints.iter().enumerate() {
        let children: Vec<String> = joints
            .iter()
            .enumerate()
            .filter(|(k, c)| *k != i && c.parent == Some(i))
            .map(|(k, _)| k.to_string())
            .collect();
        if i > 0 {
            node_json.push(',');
        }
        let [tx, ty, tz] = j.translation;
        node_json.push_str(&format!(
            "{{\"name\":{},\"translation\":[{},{},{}]",
            json_str(&j.name),
            f(tx),
            f(ty),
            f(tz)
        ));
        if !children.is_empty() {
            node_json.push_str(&format!(",\"children\":[{}]", children.join(",")));
        }
        node_json.push('}');
    }
    let roots: Vec<String> = joints
        .iter()
        .enumerate()
        .filter(|(_, j)| j.parent.is_none())
        .map(|(i, _)| i.to_string())
        .collect();

    let mut bv = String::new();
    for (i, (off, len)) in views.iter().enumerate() {
        if i > 0 {
            bv.push(',');
        }
        bv.push_str(&format!(
            "{{\"buffer\":0,\"byteOffset\":{off},\"byteLength\":{len}}}"
        ));
    }

    // Oni-specific data (root motion, combat) rides in the animation's `extras`,
    // built by the caller (it has the decoded clip).
    let extras = if extras_json.is_empty() {
        String::new()
    } else {
        format!(",\"extras\":{extras_json}")
    };

    let json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"oni-geom\"}},\
\"scene\":0,\"scenes\":[{{\"nodes\":[{}]}}],\
\"nodes\":[{node_json}],\
\"animations\":[{{\"name\":{},\"samplers\":[{samplers}],\"channels\":[{chans_json}]{extras}}}],\
\"accessors\":[{accessors}],\
\"bufferViews\":[{bv}],\
\"buffers\":[{{\"byteLength\":{}}}]}}",
        roots.join(","),
        json_str(anim_name),
        bin.len()
    );
    assemble_glb(&json, &bin)
}
