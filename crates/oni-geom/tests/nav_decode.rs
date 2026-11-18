//! Level navigation data decodes as the shipped level 1 holds it.

use std::path::PathBuf;

use oni_dat::Dat;
use oni_geom::level::TAG_AKEV;
use oni_geom::nav::{extract_navigation, quad_flag, room_flag};

#[test]
fn level1_navigation_decodes() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../original/gamedata/level1_Final.dat");
    let (Ok(bytes), Ok(raw)) = (std::fs::read(&path), std::fs::read(path.with_extension("raw"))) else {
        eprintln!("SKIP: level1 fixture not found");
        return;
    };
    let dat = Dat::parse(&bytes).expect("parse");
    let akev = dat.instances.iter().find(|i| i.tag == TAG_AKEV).expect("an environment");
    let rooms = extract_navigation(&dat, akev, &raw).expect("navigation");

    assert_eq!(rooms.len(), 240);
    assert_eq!(rooms.iter().filter(|r| r.flags & room_flag::STAIRS != 0).count(), 41);
    assert!(rooms.iter().all(|r| r.flags & room_flag::ROOM != 0 && r.planes.len() == 6));
    let links: usize = rooms.iter().map(|r| r.links.len()).sum();
    assert_eq!(links, 528);
    // Links are symmetric.
    for (i, r) in rooms.iter().enumerate() {
        for l in &r.links {
            assert!(rooms[l.to as usize].links.iter().any(|b| b.to as usize == i && b.quad == l.quad));
        }
    }
    let portal = quad_flag::GHOST | quad_flag::STAIRS_UP | quad_flag::STAIRS_DOWN;
    assert!(rooms.iter().flat_map(|r| &r.links).all(|l| l.quad_flags & portal != 0));

    // Every room has a grid; the cells add up to the counts the data holds.
    assert!(rooms.iter().all(|r| r.grid.is_some()));
    let mut counts = [0usize; 10];
    for g in rooms.iter().filter_map(|r| r.grid.as_ref()) {
        assert_eq!(g.cells.len(), (g.width * g.height) as usize);
        for &c in &g.cells {
            counts[usize::from(c)] += 1;
        }
    }
    assert_eq!(counts, [234935, 27057, 4587, 5143, 33356, 4643, 4891, 2299, 23790, 45013]);

    // The middle of each (non-stair) room's box is inside its hull.
    for r in rooms.iter().filter(|r| r.stair.is_none()) {
        let c: Vec<f32> = (0..3).map(|k| 0.5 * (r.min[k] + r.max[k])).collect();
        assert!(r.planes.iter().all(|p| p[0] * c[0] + p[1] * c[1] + p[2] * c[2] + p[3] <= 0.01));
    }
}
