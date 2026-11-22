//! Reading JSON files: the converter's, in their shared formats
//! (`oni_schema::files`), which may be missing (a level without lasers,
//! say), and the player's own.

use std::path::Path;

use serde_json::Value;

/// A JSON file's value (the player's own files: saves); `None` when it's
/// missing or doesn't parse.
pub fn read(path: impl AsRef<Path>) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
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
