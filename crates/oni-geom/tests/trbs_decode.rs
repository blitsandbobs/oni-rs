//! Validates TRBS character decoding on level0: bodies extract with parts, a
//! valid parent hierarchy, and a well-formed multi-node `.glb`.

use std::path::PathBuf;

use oni_dat::Dat;
use oni_geom::character::{bodies, lod};
use oni_geom::glb::{scene_to_glb, SceneNode};

fn level0() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ONI_DAT_LEVEL0") {
        let p = PathBuf::from(p);
        return p.exists().then_some(p);
    }
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("original/gamedata/level0_Final.dat");
    p.exists().then_some(p)
}

#[test]
fn level0_characters_decode() {
    let Some(path) = level0() else {
        eprintln!("SKIP: level0 fixture not found");
        return;
    };
    let bytes = std::fs::read(&path).expect("read");
    let dat = Dat::parse(&bytes).expect("parse");

    let mut count = 0usize;
    let mut total_parts = 0usize;
    for (_, body) in bodies(&dat, lod::HIGH) {
        assert!(!body.parts.is_empty(), "body has no parts");
        // Part 0 is the root; every other parent index is in range and not self.
        assert!(body.parts[0].parent.is_none(), "root should have no parent");
        for (i, p) in body.parts.iter().enumerate() {
            if let Some(par) = p.parent {
                assert!(par < body.parts.len(), "parent in range");
                assert_ne!(par, i, "no self-parent");
            }
            assert!(!p.mesh.positions.is_empty(), "part mesh has geometry");
        }
        total_parts += body.parts.len();
        count += 1;
    }
    assert!(count > 10, "expected several characters, got {count}");
    eprintln!("decoded {count} character bodies, {total_parts} parts total");

    // Build a scene glb from the first body and check the glTF header.
    let (_, body) = bodies(&dat, lod::HIGH).next().expect("a body");
    let nodes: Vec<SceneNode> = body
        .parts
        .iter()
        .map(|p| SceneNode {
            name: p.bone.clone(),
            translation: p.translation,
            rotation: p.rotation,
            parent: p.parent,
            mesh: Some(&p.mesh),
            blend: false,
            additive: false,
            self_illum: false,
            texture_uri: None,
        })
        .collect();
    let glb = scene_to_glb(&nodes);
    assert_eq!(&glb[0..4], b"glTF");
    assert_eq!(
        u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize,
        glb.len()
    );
}

#[test]
fn animation_decodes_all_frames() {
    let Some(path) = level0() else {
        eprintln!("SKIP: level0 fixture not found");
        return;
    };
    let raw_path = path.with_extension("raw");
    let Ok(raw) = std::fs::read(&raw_path) else {
        eprintln!("SKIP: level0 .raw not found");
        return;
    };
    let bytes = std::fs::read(&path).expect("read");
    let dat = Dat::parse(&bytes).expect("parse");

    let anim = oni_geom::character::find_animation(&dat, "NINJArun1stepa")
        .expect("NINJArun1stepa animation");
    let clip = oni_geom::character::decode_animation(&dat, anim, &raw).expect("decode clip");

    assert_eq!(clip.channels.len(), 19, "biped has 19 bone channels");
    assert!(clip.num_frames > 1, "clip has multiple frames");
    assert_eq!(clip.fps, 60.0);
    for ch in &clip.channels {
        assert!(!ch.times.is_empty(), "channel has keyframes");
        assert_eq!(
            ch.times.len(),
            ch.rotations.len(),
            "times and rotations align"
        );
        // times are non-decreasing and start at 0
        assert_eq!(ch.times[0], 0.0);
        assert!(
            ch.times.windows(2).all(|w| w[1] >= w[0]),
            "times non-decreasing"
        );
        // every rotation is ~unit length
        for q in &ch.rotations {
            let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
            assert!((n - 1.0).abs() < 1e-3, "unit quaternion");
        }
    }
    // Root motion: one local [x,z] per frame; a run accumulates forward (z).
    assert_eq!(
        clip.root_motion.len(),
        clip.num_frames as usize,
        "root motion has one sample per frame"
    );
    let forward: f32 = clip.root_motion.iter().map(|p| p[1]).sum();
    assert!(
        forward > 1.0,
        "run accumulates forward z motion, got {forward}"
    );

    // An attack animation carries a hit window with damage.
    if let Some(kick) = oni_geom::character::find_animation(&dat, "NINCOMkick_heavy") {
        let a = oni_geom::character::decode_animation(&dat, kick, &raw).expect("decode kick");
        let atk = a.attacks.first().expect("kick has an attack");
        assert!(atk.damage > 0, "attack deals damage");
        assert!(
            atk.first_damage_frame < atk.last_damage_frame,
            "valid hit window"
        );
        assert!(
            (atk.last_damage_frame as u32) < a.num_frames,
            "hit window within the clip"
        );
    }
    eprintln!(
        "decoded {}-frame clip, {} channels, forward root motion {forward:.1}",
        clip.num_frames,
        clip.channels.len()
    );
}

#[test]
fn level_environment_decodes() {
    // Level 1 holds the AKEV environment; level0 is global data.
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("original/gamedata/level1_Final.dat");
    if !p.exists() {
        eprintln!("SKIP: level1 fixture not found");
        return;
    }
    let bytes = std::fs::read(&p).expect("read");
    let dat = Dat::parse(&bytes).expect("parse");
    let akev = dat
        .instances
        .iter()
        .find(|i| i.tag == oni_geom::level::TAG_AKEV)
        .expect("an AKEV environment");
    let level = oni_geom::level::extract_level(&dat, akev).expect("extract level");

    assert!(
        level.environment.len() > 50,
        "many material groups, got {}",
        level.environment.len()
    );
    assert!(
        level.objects.len() > 100,
        "placed objects separated out, got {}",
        level.objects.len()
    );
    let mut tris = 0usize;
    let all_groups = level
        .environment
        .iter()
        .chain(level.objects.iter().flat_map(|o| o.groups.iter()));
    for g in all_groups {
        assert_eq!(g.positions.len(), g.texcoords.len());
        assert_eq!(g.positions.len(), g.normals.len());
        assert_eq!(g.indices.len() % 3, 0);
        for &i in &g.indices {
            assert!((i as usize) < g.positions.len());
        }
        tris += g.indices.len() / 3;
    }
    assert!(tris > 10000, "substantial geometry, got {tris} tris");
    // Objects carry a type and a stable node name.
    assert!(level.objects.iter().any(|o| o.type_name == "furniture"));
    eprintln!(
        "level: {} env groups, {} objects, {tris} triangles",
        level.environment.len(),
        level.objects.len()
    );
}

#[test]
fn object_animations_decode() {
    use oni_geom::objanim::{decode_object_animation, TAG_OBAN};
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("original/gamedata/level1_Final.dat");
    if !p.exists() {
        eprintln!("SKIP: level1 fixture not found");
        return;
    }
    let bytes = std::fs::read(&p).expect("read");
    let dat = Dat::parse(&bytes).expect("parse");
    let mut count = 0;
    let mut total_keys = 0;
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_OBAN) {
        let a = decode_object_animation(&dat, inst).expect("decode OBAN");
        // Keyframes are ordered and quaternions are near-unit.
        for k in &a.keys {
            let [x, y, z, w] = k.rotation;
            let m = (x * x + y * y + z * z + w * w).sqrt();
            assert!((0.9..1.1).contains(&m), "unit quat, got {m}");
        }
        total_keys += a.keys.len();
        count += 1;
    }
    assert!(count > 10, "many object animations, got {count}");
    assert!(total_keys > count, "clips carry keyframes");
    eprintln!("decoded {count} OBAN clips, {total_keys} keyframes");
}

#[test]
fn character_classes_decode() {
    use oni_geom::charclass::{character_classes, decode_character_class, TAG_ONCC};
    // level0 holds the global character classes.
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("original/gamedata/level0_Final.dat");
    if !p.exists() {
        eprintln!("SKIP: level0 fixture not found");
        return;
    }
    let bytes = std::fs::read(&p).expect("read");
    let dat = Dat::parse(&bytes).expect("parse");

    let classes: Vec<_> = character_classes(&dat).collect();
    assert!(classes.len() > 50, "many classes, got {}", classes.len());
    // Most classes resolve a body and an animation set via the ref-scan.
    let with_body = classes.iter().filter(|c| c.body.is_some()).count();
    let with_anims = classes.iter().filter(|c| !c.animations.is_empty()).count();
    assert!(with_body * 10 >= classes.len() * 9, "most have a body");
    assert!(with_anims * 10 >= classes.len() * 9, "most have animations");

    // Konoko resolves to her body, variant and a non-trivial animation set.
    let konoko = dat
        .instances
        .iter()
        .find(|i| i.tag == TAG_ONCC && i.name.as_deref() == Some("konoko_generic"))
        .expect("konoko_generic ONCC");
    let cc = decode_character_class(&dat, konoko).expect("decode");
    assert_eq!(cc.body.as_deref(), Some("konoko_body_high"));
    assert_eq!(cc.variant.as_deref(), Some("konoko"));
    assert!(
        cc.animations.len() > 100,
        "konoko anim set, got {}",
        cc.animations.len()
    );
    assert!(cc.animations.iter().any(|a| a.starts_with("KONOKO")));

    // Gameplay scalars read from the stable tail (anchored on the body ref).
    let s = cc.stats.expect("konoko stats");
    assert_eq!(s.max_hit_points, 200, "konoko HP");
    assert!(s.scale_high >= s.scale_low && s.scale_low > 0.0);
    assert!(s.damage_resistance[2] > 0.0 && s.damage_resistance[3] > 0.0);
    let with_stats = classes.iter().filter(|c| c.stats.is_some()).count();
    assert!(with_stats * 10 >= classes.len() * 9, "most have stats");
    eprintln!(
        "decoded {} classes; konoko: {} animations, {} HP",
        classes.len(),
        cc.animations.len(),
        s.max_hit_points
    );
}
