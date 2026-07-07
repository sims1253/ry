//! Built-in database of base R function signatures, embedded at compile time.
//!
//! Signatures are intentionally underspecified: many use abstract slots
//! like "arg0" (type mirrors the first positional argument) or "unknown".
//! The checker resolves these slots when applying a signature.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const BASE_JSON: &str = include_str!("../data/base.json");
pub const DPLYR_JSON: &str = include_str!("../data/dplyr.json");
pub const PURRR_JSON: &str = include_str!("../data/purrr.json");
pub const MIRAI_JSON: &str = include_str!("../data/mirai.json");
pub const BAYES_JSON: &str = include_str!("../data/bayes.json");
pub const SURVIVAL_JSON: &str = include_str!("../data/survival.json");

/// The set of packages whose type signatures ship embedded in ry.
/// `base` is always attached (returned by `load_base_cached`); the
/// others are attached on demand when `library(pkg)` is recorded or a
/// `pkg::`-qualified call is resolved. `bayes` is a multi-package
/// typeshed whose JSON keys are `pkg.function` (brms, posterior, loo,
/// bayesplot, cmdstanr); see [`load_package`].
pub const KNOWN_PACKAGES: &[&str] = &["base", "dplyr", "purrr", "mirai", "bayes", "survival"];

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
    let raw: RawFile = serde_json::from_str(BASE_JSON)?;
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

/// Parse a package typeshed JSON document into a `Typeshed`. `prefix`
/// strips a leading `<prefix>.` from each function key (used by the
/// multi-package `bayes.json`, whose keys are `brms.brm`,
/// `posterior.as_draws_df`, etc.) so callers see the bare function name
/// (`brm`, `as_draws_df`) for the requested package.
fn parse_package(json: &str, prefix: Option<&str>) -> Result<Typeshed, TypeshedError> {
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
    let raw: RawFile = serde_json::from_str(json)?;
    let strip = |k: &str| -> String {
        match prefix {
            Some(p) => k
                .strip_prefix(&format!("{p}."))
                .map(|s| s.to_string())
                .unwrap_or_else(|| k.to_string()),
            None => k.to_string(),
        }
    };
    let mut functions = std::collections::BTreeMap::new();
    for (k, v) in raw.functions {
        functions.insert(
            strip(&k),
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

/// Load a non-base package's typeshed. Returns `None` for an unknown
/// package name (the checker treats that as "no signatures available",
/// i.e. opaque). Known packages and their JSON sources:
///
/// - `dplyr` -> `data/dplyr.json` (bare function names).
/// - `purrr` -> `data/purrr.json` (bare function names).
/// - `mirai` -> `data/mirai.json` (bare function names).
/// - `survival` -> `data/survival.json` (bare function names).
/// - `brms`, `posterior`, `loo`, `bayesplot`, `cmdstanr` ->
///   `data/bayes.json` (a single multi-package file whose keys are
///   `pkg.function`; the prefix is stripped for the requested package).
///
/// Results are cached for the life of the process (the JSON documents
/// are compile-time-embedded and never change), so repeated lookups are
/// cheap.
pub fn load_package(name: &str) -> Option<&'static Typeshed> {
    static DPLYR: std::sync::OnceLock<Typeshed> = std::sync::OnceLock::new();
    static PURRR: std::sync::OnceLock<Typeshed> = std::sync::OnceLock::new();
    static MIRAI: std::sync::OnceLock<Typeshed> = std::sync::OnceLock::new();
    static SURVIVAL: std::sync::OnceLock<Typeshed> = std::sync::OnceLock::new();
    static BRMS: std::sync::OnceLock<Typeshed> = std::sync::OnceLock::new();
    static POSTERIOR: std::sync::OnceLock<Typeshed> = std::sync::OnceLock::new();
    static LOO: std::sync::OnceLock<Typeshed> = std::sync::OnceLock::new();
    static BAYESPLOT: std::sync::OnceLock<Typeshed> = std::sync::OnceLock::new();
    static CMDSTANR: std::sync::OnceLock<Typeshed> = std::sync::OnceLock::new();

    let (cache, json, prefix): (
        &'static std::sync::OnceLock<Typeshed>,
        &'static str,
        Option<&'static str>,
    ) = match name {
        "dplyr" => (&DPLYR, DPLYR_JSON, None),
        "purrr" => (&PURRR, PURRR_JSON, None),
        "mirai" => (&MIRAI, MIRAI_JSON, None),
        "survival" => (&SURVIVAL, SURVIVAL_JSON, None),
        "brms" => (&BRMS, BAYES_JSON, Some("brms")),
        "posterior" => (&POSTERIOR, BAYES_JSON, Some("posterior")),
        "loo" => (&LOO, BAYES_JSON, Some("loo")),
        "bayesplot" => (&BAYESPLOT, BAYES_JSON, Some("bayesplot")),
        "cmdstanr" => (&CMDSTANR, BAYES_JSON, Some("cmdstanr")),
        _ => return None,
    };
    Some(
        cache.get_or_init(|| {
            parse_package(json, prefix).expect("embedded package typeshed must parse")
        }),
    )
}

/// Whether a package name is known to ry's embedded typeshed. Used by
/// the checker to decide whether a `library(pkg)` contributes
/// signatures (unknown packages are still recorded as loaded for NSE
/// gating, e.g. `tidyverse`, but contribute no function signatures).
pub fn is_known_package(name: &str) -> bool {
    matches!(
        name,
        "dplyr"
            | "purrr"
            | "mirai"
            | "survival"
            | "brms"
            | "posterior"
            | "loo"
            | "bayesplot"
            | "cmdstanr"
    )
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

    #[test]
    fn load_package_dplyr_has_filter() {
        let t = load_package("dplyr").expect("dplyr is a known package");
        assert!(t.functions.contains_key("filter"));
        assert!(t.functions.contains_key("mutate"));
    }

    #[test]
    fn load_package_purrr_has_map_family() {
        let t = load_package("purrr").expect("purrr is a known package");
        assert!(t.functions.contains_key("map"));
        assert!(t.functions.contains_key("map_dbl"));
        assert!(t.functions.contains_key("in_parallel"));
    }

    #[test]
    fn load_package_survival_has_survfit() {
        let t = load_package("survival").expect("survival is a known package");
        assert!(t.functions.contains_key("Surv"));
        assert!(t.functions.contains_key("survfit"));
        assert!(
            t.s3_methods
                .contains_key(&("quantile".to_string(), "survfit".to_string()))
        );
    }

    #[test]
    fn load_package_bayes_strips_prefix() {
        // bayes.json is a multi-package file keyed `pkg.function`; each
        // package view should expose bare function names.
        let brms = load_package("brms").expect("brms is known");
        assert!(brms.functions.contains_key("brm"));
        assert!(!brms.functions.contains_key("brms.brm"));
        assert!(brms.functions.contains_key("posterior_predict"));

        let posterior = load_package("posterior").expect("posterior is known");
        assert!(posterior.functions.contains_key("as_draws_df"));
        assert!(!posterior.functions.contains_key("posterior.as_draws_df"));
        // The brms-only view must NOT see posterior's entries.
        assert!(!brms.functions.contains_key("as_draws_df"));
    }

    #[test]
    fn load_package_unknown_returns_none() {
        assert!(load_package("doesnotexist").is_none());
    }

    #[test]
    fn is_known_package_recognises_known() {
        assert!(is_known_package("dplyr"));
        assert!(is_known_package("survival"));
        assert!(is_known_package("brms"));
        assert!(is_known_package("posterior"));
        assert!(!is_known_package("tidyverse"));
        assert!(!is_known_package("doesnotexist"));
    }
}
