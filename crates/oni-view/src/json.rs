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

/// A number as `f32` (0 when absent).
pub fn f32(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

/// `[x, y, z]` (absent parts 0).
pub fn vec3(v: &Value) -> Vec3 {
    Vec3::new(f32(&v[0]), f32(&v[1]), f32(&v[2]))
}
