//! Our own fixes to the shipped level scripts' most glaring mistakes
//! (`docs/deviations.md`): per level, a Lua file run after the translated
//! scripts, replacing script functions through `FIX` (`prelude.lua`). Off
//! with `--faithful`.

/// The fixes for level `NN`, if it has any.
pub fn source(level: &str) -> Option<&'static str> {
    FILES.iter().find(|(l, _)| *l == level).map(|(_, s)| *s)
}

const FILES: &[(&str, &str)] = &[
    ("02", include_str!("fixes/02.lua")),
    ("04", include_str!("fixes/04.lua")),
    ("12", include_str!("fixes/12.lua")),
    ("18", include_str!("fixes/18.lua")),
];
