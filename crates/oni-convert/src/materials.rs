//! Materials: which textures break like glass.
//!
//! Each material (`Mtrl` template) names its parent; together they form a
//! tree. A material is breakable if it is `Glass` or below it — unless it
//! is `Unbreak_Glass` or below that (the original works this out at load,
//! `BFW_Materials.c`). Textures get their material from a table in level 0:
//! the binary data `TMBD` ("TextureMaterials"; `Oni_TextureMaterials.c`) —
//! a u32 version, a u32 count, then per entry a material name and a texture
//! name, 32 bytes each. (The texture's own material field is filled from it
//! at run time; on disk it is always 0.)

use std::collections::{BTreeSet, HashMap};

use oni_dat::{Dat, FourCc};

use crate::{cstr, le_u32};

const TAG_MTRL: FourCc = FourCc(u32::from_be_bytes(*b"Mtrl"));
const TAG_BINA: FourCc = FourCc(u32::from_be_bytes(*b"BINA"));
const NAME: usize = 32;

/// Whether material `m` breaks, by the parent chain (`parents`: name →
/// parent name).
fn breaks(m: &str, parents: &HashMap<String, Option<String>>) -> bool {
    let mut at = Some(m.to_string());
    for _ in 0..64 {
        match at.as_deref() {
            Some("Glass") => return true,
            Some("Unbreak_Glass") | None => return false,
            Some(n) => at = parents.get(n).cloned().flatten(),
        }
    }
    false
}

/// The breakable material names among `dat`'s materials.
pub fn breakable(dat: &Dat<'_>) -> BTreeSet<String> {
    let name_of = |k: usize| dat.instances.get(k).and_then(|i| i.name.as_deref()).map(|n| n.trim_start_matches("Mtrl").to_string());
    let parents: HashMap<String, Option<String>> = dat
        .instances
        .iter()
        .filter(|i| i.tag == TAG_MTRL)
        .filter_map(|i| {
            let d = dat.instance_data(i)?;
            let name = i.name.as_deref()?.trim_start_matches("Mtrl").to_string();
            let r = le_u32(d, 8);
            Some((name, (r & 1 != 0).then(|| name_of((r >> 8) as usize)).flatten()))
        })
        .collect();
    parents.keys().filter(|m| breaks(m, &parents)).cloned().collect()
}

/// The texture → material table (texture name, material name).
pub fn texture_materials(dat: &Dat<'_>, raw: &[u8]) -> Vec<(String, String)> {
    let Some(inst) = dat.instances.iter().find(|i| i.tag == TAG_BINA && i.name.as_deref().is_some_and(|n| n.starts_with("DBMT"))) else {
        return Vec::new();
    };
    let Some(bd) = dat.instance_data(inst) else { return Vec::new() };
    // The binary data's payload, after its 8-byte header (class, size).
    let start = le_u32(bd, 4) as usize + 8;
    let Some(p) = raw.get(start..) else { return Vec::new() };
    if p.len() < 8 {
        return Vec::new();
    }
    let n = le_u32(p, 4) as usize;
    (0..n)
        .filter_map(|k| {
            let o = 8 + k * 2 * NAME;
            (o + 2 * NAME <= p.len()).then(|| (cstr(p, o + NAME, NAME), cstr(p, o, NAME)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glass_breaks_below_it_unbreakable_glass_does_not() {
        let tree = [("Base", None), ("Glass", Some("Base")), ("Pane", Some("Glass")), ("Unbreak_Glass", Some("Glass")), ("Thick", Some("Unbreak_Glass")), ("Wood", Some("Base"))];
        let parents: HashMap<String, Option<String>> = tree.iter().map(|(n, p)| (n.to_string(), p.map(String::from))).collect();
        let got: Vec<bool> = tree.iter().map(|(n, _)| breaks(n, &parents)).collect();
        assert_eq!(got, [false, true, true, false, false, false]);
    }
}
