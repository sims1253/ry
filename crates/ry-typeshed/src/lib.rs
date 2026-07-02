//! Built-in database of base R function signatures, embedded at compile time.
//!
//! Signatures are intentionally underspecified: many use abstract slots
//! like "arg0" (type mirrors the first positional argument) or "unknown".
//! The checker resolves these slots when applying a signature.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const BASE_R_JSON: &str = include_str!("../data/base_r.json");

#[derive(Debug, Error)]
pub enum TypeshedError {
    #[error("typeshed parse error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Abstract slot for a return type that depends on the call's arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnTypeSlot {
    /// Literally as written in the JSON: a free-form token that the
    /// checker knows how to interpret ("arg0", "longest_arg", etc.).
    Slot(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct JsonRType {
    pub mode: String,
    pub length: String,
    #[serde(default)]
    pub na: bool,
    /// S3 class vector, e.g. `["data.frame"]` for `mtcars`. Default
    /// empty for backward compatibility with existing JSON.
    #[serde(default)]
    pub class: Vec<String>,
    /// Named column schema for record-like values (data frames and
    /// lists with a known shape). Empty for non-record values. The
    /// checker reads this from the dataset entry directly; the
    /// `Typeshed` struct itself does not need a parallel field.
    #[serde(default)]
    pub columns: std::collections::BTreeMap<String, JsonRType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ReturnSpec {
    /// Free-form slot, e.g. "arg0" or "concat_of_args".
    Slot(String),
    /// A concrete type spec.
    Concrete(JsonRType),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FunctionSig {
    pub params: Vec<String>,
    pub return_: ReturnSpec,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FunctionSigWithKey {
    pub name: String,
    #[serde(flatten)]
    pub sig: FunctionSig,
}

impl FunctionSig {
    pub fn params(&self) -> &[String] {
        &self.params
    }

    pub fn return_(&self) -> &ReturnSpec {
        &self.return_
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Typeshed {
    pub version: String,
    pub functions: std::collections::BTreeMap<String, FunctionSig>,
    /// Built-in datasets (`mtcars`, `iris`, ...) typed as list-typed
    /// values. These resolve in the checker when an identifier is not
    /// bound by user code or function scope.
    #[serde(default)]
    pub datasets: std::collections::BTreeMap<String, JsonRType>,
    /// Built-in S3 methods keyed by `(generic, class)`. The checker
    /// consults this during S3 dispatch; the presence of a `default`
    /// entry for a generic suppresses RY050 for any class without a
    /// more specific method.
    #[serde(default)]
    pub s3_methods: std::collections::BTreeMap<(String, String), FunctionSig>,
}

/// Wrapper to handle the JSON shape where the key "return" is reserved
/// (it's a Rust keyword). We deserialize via a serde alias.
mod _fwd {
    use serde::{Deserialize, Serialize};
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
    pub struct _FunctionSig {
        pub params: Vec<String>,
        #[serde(rename = "return")]
        pub return_: super::ReturnSpec,
        #[serde(default)]
        pub aliases: Vec<String>,
    }
}

/// JSON shape for a single S3 method entry in `base_r.json`. The
/// `(generic, class)` pair becomes the BTreeMap key after deserialization.
#[derive(Debug, Clone, Deserialize)]
struct RawS3Method {
    generic: String,
    class: String,
    #[serde(flatten)]
    sig: _fwd::_FunctionSig,
}

pub fn load_base() -> Result<Typeshed, TypeshedError> {
    // Use intermediate structs because serde derive can't directly rename
    // `return` to `return_` inside BTreeMap values without a custom impl.
    #[derive(serde::Deserialize)]
    struct RawFile {
        #[allow(dead_code)]
        version: String,
        functions: std::collections::BTreeMap<String, _fwd::_FunctionSig>,
        #[serde(default)]
        datasets: std::collections::BTreeMap<String, JsonRType>,
        #[serde(default)]
        s3_methods: Vec<RawS3Method>,
    }
    let raw: RawFile = serde_json::from_str(BASE_R_JSON)?;
    let mut functions = std::collections::BTreeMap::new();
    for (k, v) in raw.functions {
        functions.insert(
            k,
            FunctionSig {
                params: v.params,
                return_: v.return_,
                aliases: v.aliases,
            },
        );
    }
    let mut s3_methods = std::collections::BTreeMap::new();
    for m in raw.s3_methods {
        let key = (m.generic, m.class);
        s3_methods.insert(
            key,
            FunctionSig {
                params: m.sig.params,
                return_: m.sig.return_,
                aliases: m.sig.aliases,
            },
        );
    }
    Ok(Typeshed {
        version: env!("CARGO_PKG_VERSION").to_string(),
        functions,
        datasets: raw.datasets,
        s3_methods,
    })
}

/// Load the base typeshed once and cache it for the life of the process.
///
/// The base typeshed is a compile-time-embedded 61KB JSON document that
/// never changes after startup. Parsing it on every `Checker::new` (which
/// happens once per file in a `Project`, and once per keystroke in the
/// LSP) is pure waste. This caches the parsed value in a `OnceLock` so the
/// JSON is deserialized exactly once; subsequent callers receive a
/// reference to the cached `Typeshed`.
///
/// Callers that mutate the typeshed (none do today, but the API allows it)
/// should `.clone()` the returned reference rather than mutating the cache.
pub fn load_base_cached() -> Result<&'static Typeshed, TypeshedError> {
    static CACHE: std::sync::OnceLock<Typeshed> = std::sync::OnceLock::new();
    // `get_or_try_init` is still unstable, so initialize eagerly via
    // `get_or_init`. The base typeshed is a compile-time-embedded JSON
    // document that always parses; a failure here is a build-time data
    // bug, not a runtime condition, so panicking during first access is
    // acceptable (and matches the existing `load_base().expect()` callers).
    if let Some(cached) = CACHE.get() {
        return Ok(cached);
    }
    let typeshed = load_base()?;
    // Another thread may have raced us; `set` returns the winner's value.
    let cached = match CACHE.set(typeshed) {
        Ok(()) => CACHE.get().expect("cache just set"),
        Err(loser) => {
            // We lost the race; the winner's value is already in the cache.
            let _ = loser;
            CACHE.get().expect("cache set by racing thread")
        }
    };
    Ok(cached)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_json_loads() {
        let t = load_base().expect("loads");
        assert!(t.functions.contains_key("length"));
        assert!(t.functions.contains_key("c"));
        assert!(t.functions.len() >= 40, "typeshed has at least 40 entries");
    }
}
