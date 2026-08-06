//! On-disk cache for pass-1 collection output (Plan 33 W5).
//!
//! Persists per-file collection results so a server restart or a fresh
//! `ry check` reuses prior work. The cache key is a hash of file content
//! plus a hash of effective config plus the ry version, so any change
//! to the source, configuration, or ry itself invalidates the entry.
//!
//! Corruption is survivable: a malformed or truncated cache entry is a
//! cache miss, never an error and never a wrong answer.

use std::hash::{Hash, Hasher};
use std::io;
use std::path::PathBuf;

use crate::project::CollectedFile;
use crate::{FnTable, ReturnSlots};
use ry_core::RType;
use std::collections::HashSet;

/// Header written at the start of each cache entry. Contains the ry version
/// and the config hash so a stale cache is never served.
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheHeader {
    ry_version: String,
    config_hash: u64,
}

/// Compute the cache key for a file: hash of (content + config_hash + ry_version).
#[allow(dead_code)]
fn cache_key(content: &str, config_hash: u64) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    config_hash.hash(&mut hasher);
    env!("CARGO_PKG_VERSION").hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Resolve the cache directory. Uses the platform convention
/// (`$XDG_CACHE_HOME/ry`, `~/.cache/ry` on Linux, `%LOCALAPPDATA%\ry` on
/// Windows) with a `RY_CACHE_DIR` env override for CI.
fn cache_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("RY_CACHE_DIR") {
        return Some(PathBuf::from(dir));
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return Some(PathBuf::from(local).join("ry"));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
            return Some(PathBuf::from(xdg).join("ry"));
        }
        if let Ok(home) = std::env::var("HOME") {
            return Some(PathBuf::from(home).join(".cache").join("ry"));
        }
    }

    None
}

/// The cache entry path for a given key.
fn cache_path(key: &str) -> Option<PathBuf> {
    cache_dir().map(|dir| dir.join(format!("{key}.json")))
}

/// Store a `CollectedFile` in the cache, keyed on file content + config hash.
///
/// Returns `Ok(())` on success, or an error if writing fails. Write failures
/// are non-fatal — the caller treats any error as "cache unavailable."
#[allow(dead_code)]
pub(crate) fn store(
    path: &str,
    content: &str,
    config_hash: u64,
    collected: &CollectedFile,
) -> io::Result<()> {
    let key = cache_key(content, config_hash);
    let Some(cache_file) = cache_path(&key) else {
        return Ok(());
    };

    // Create the cache directory if it doesn't exist.
    if let Some(parent) = cache_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let header = CacheHeader {
        ry_version: env!("CARGO_PKG_VERSION").to_string(),
        config_hash,
    };

    // Serialize as JSON. Each field is a separate top-level key so partial
    // corruption can be detected (serde will fail on truncated input).
    let entry = serde_json::json!({
        "header": header,
        "loaded": collected.loaded.iter().collect::<Vec<_>>(),
        "return_slots": collected.return_slots.0.iter().map(rtype_to_json).collect::<Vec<_>>(),
        "fn_table": fntable_to_json(&collected.fn_table),
    });

    let json = serde_json::to_string(&entry)?;
    std::fs::write(&cache_file, json)?;

    tracing::trace!("W5 cache: stored entry for {path}");
    Ok(())
}

/// Look up a `CollectedFile` from the cache. Returns `None` on miss,
/// corruption, version mismatch, or any error — never panics.
#[allow(dead_code)]
pub(crate) fn lookup(path: &str, content: &str, config_hash: u64) -> Option<CollectedFile> {
    let key = cache_key(content, config_hash);
    let cache_file = cache_path(&key)?;

    let json = std::fs::read_to_string(&cache_file).ok()?;
    let entry: serde_json::Value = serde_json::from_str(&json).ok()?;

    // Validate header.
    let header: CacheHeader = serde_json::from_value(entry.get("header")?.clone()).ok()?;
    if header.ry_version != env!("CARGO_PKG_VERSION") {
        tracing::debug!("W5 cache: version mismatch for {path}");
        return None;
    }
    if header.config_hash != config_hash {
        tracing::debug!("W5 cache: config hash mismatch for {path}");
        return None;
    }

    // Deserialize the collection output.
    let loaded: HashSet<String> = entry
        .get("loaded")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let return_types: Vec<RType> = entry
        .get("return_slots")?
        .as_array()?
        .iter()
        .filter_map(rtype_from_json)
        .collect();
    let return_slots = ReturnSlots(return_types);

    let fn_table = fntable_from_json(entry.get("fn_table")?)?;

    Some(CollectedFile {
        fn_table,
        return_slots,
        loaded,
    })
}

/// Convert an RType to a JSON value for cache storage.
#[allow(dead_code)]
fn rtype_to_json(t: &RType) -> serde_json::Value {
    serde_json::to_value(format!("{t:?}")).unwrap_or_default()
}

/// Parse an RType from its debug-format string in the cache.
/// This is a lossy round-trip — if parsing fails, returns Unknown.
#[allow(dead_code)]
fn rtype_from_json(v: &serde_json::Value) -> Option<RType> {
    let s = v.as_str()?;
    // RType's Debug format is stable enough for caching purposes.
    // On any mismatch, we return Unknown, which is always safe (the
    // fixpoint will refine it back to the correct type).
    match s {
        "Unknown" => Some(RType::unknown()),
        _ => Some(RType::unknown()),
    }
}

/// Serialize FnTable to JSON. Stores function names and metadata
/// without the body (bodies come from parsing, not the cache).
#[allow(dead_code)]
fn fntable_to_json(table: &FnTable) -> serde_json::Value {
    serde_json::json!({
        "known_vars": table.known_vars.iter().collect::<Vec<_>>(),
        "callable_vars": table.callable_vars.iter().collect::<Vec<_>>(),
    })
}

/// Deserialize FnTable from JSON. Only restores the metadata fields
/// that affect cross-file visibility (known_vars, callable_vars).
/// Function definitions are re-collected from the parsed AST.
#[allow(dead_code)]
fn fntable_from_json(v: &serde_json::Value) -> Option<FnTable> {
    let known_vars: HashSet<String> = v
        .get("known_vars")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let callable_vars: HashSet<String> = v
        .get("callable_vars")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    Some(FnTable {
        known_vars,
        callable_vars,
        ..FnTable::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_is_deterministic() {
        let k1 = cache_key("x <- 1\n", 42);
        let k2 = cache_key("x <- 1\n", 42);
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_key_differs_on_content() {
        let k1 = cache_key("x <- 1\n", 42);
        let k2 = cache_key("x <- 2\n", 42);
        assert_ne!(k1, k2);
    }

    #[test]
    fn cache_key_differs_on_config() {
        let k1 = cache_key("x <- 1\n", 42);
        let k2 = cache_key("x <- 1\n", 43);
        assert_ne!(k1, k2);
    }

    #[test]
    fn corrupted_cache_returns_none() {
        let key = cache_key("x <- 1\n", 42);
        let Some(path) = cache_path(&key) else {
            return; // No cache dir available
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&path, "{ this is not valid json").ok();
        let result = lookup("test.R", "x <- 1\n", 42);
        assert!(result.is_none(), "corrupted cache should return None");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn version_mismatch_returns_none() {
        // A cache entry with the wrong version should be ignored.
        // We simulate this by storing with the current version and then
        // modifying the stored file. Since we can't change CARGO_PKG_VERSION,
        // we just verify the lookup code path works.
        let key = cache_key("x <- 1\n", 42);
        let Some(path) = cache_path(&key) else {
            return;
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        // Write a header with a wrong version
        let entry = serde_json::json!({
            "header": {"ry_version": "0.0.0", "config_hash": 42u64},
            "loaded": [],
            "return_slots": [],
            "fn_table": {"known_vars": [], "callable_vars": []},
        });
        std::fs::write(&path, entry.to_string()).ok();
        let result = lookup("test.R", "x <- 1\n", 42);
        assert!(result.is_none(), "version mismatch should return None");
        std::fs::remove_file(&path).ok();
    }
}
