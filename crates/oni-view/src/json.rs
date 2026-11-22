//! Reading the converter's JSON: files that may be missing (a level
//! without lasers, say) and the few value shapes it writes everywhere.

use std::path::Path;

use glam::Vec3;
use serde_json::Value;

/// A JSON file's value; `None` when it's missing or doesn't parse.
pub fn read(path: impl AsRef<Path>) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// A JSON file's value; `Null` when it's missing or doesn't parse.
pub fn load(path: impl AsRef<Path>) -> Value {
    read(path).unwrap_or_default()
}

/// A converted file in its shared format (`oni_schema::files`): its
/// default when the file is missing (a level without lasers, say); a file
/// there that doesn't read as its format is said so, and taken as missing.
pub fn read_file<T: serde::de::DeserializeOwned + Default>(path: impl AsRef<Path>) -> T {
    read_some(path).unwrap_or_default()
}

/// A converted file in its shared format; `None` when it is missing, or
/// (said so) when it doesn't read as its format.
pub fn read_some<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> Option<T> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text)
        .map_err(|e| eprintln!("{}: {e}", path.display()))
        .ok()
}

/// A number as `f32` (0 when absent).
pub fn f32(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

/// `[x, y, z]` (absent parts 0).
pub fn vec3(v: &Value) -> Vec3 {
    Vec3::new(f32(&v[0]), f32(&v[1]), f32(&v[2]))
}
