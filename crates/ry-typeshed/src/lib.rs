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

pub fn load_base() -> Result<Typeshed, TypeshedError> {
    // Use intermediate structs because serde derive can't directly rename
    // `return` to `return_` inside BTreeMap values without a custom impl.
    #[derive(serde::Deserialize)]
    struct RawFile {
        #[allow(dead_code)]
        version: String,
        functions: std::collections::BTreeMap<String, _fwd::_FunctionSig>,
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
    Ok(Typeshed {
        version: env!("CARGO_PKG_VERSION").to_string(),
        functions,
    })
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
