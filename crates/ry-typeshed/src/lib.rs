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

#[derive(Clone, Copy)]
struct PackageSpec {
    name: &'static str,
    json: &'static str,
    prefix: Option<&'static str>,
}

/// Single source of truth for embedded non-base packages, in signature
/// resolution order. Bayesian packages share a JSON document but remain
/// distinct namespaces through their prefixes.
const PACKAGE_SPECS: &[PackageSpec] = &[
    PackageSpec {
        name: "dplyr",
        json: DPLYR_JSON,
        prefix: None,
    },
    PackageSpec {
        name: "purrr",
        json: PURRR_JSON,
        prefix: None,
    },
    PackageSpec {
        name: "mirai",
        json: MIRAI_JSON,
        prefix: None,
    },
    PackageSpec {
        name: "survival",
        json: SURVIVAL_JSON,
        prefix: None,
    },
    PackageSpec {
        name: "brms",
        json: BAYES_JSON,
        prefix: Some("brms"),
    },
    PackageSpec {
        name: "posterior",
        json: BAYES_JSON,
        prefix: Some("posterior"),
    },
    PackageSpec {
        name: "loo",
        json: BAYES_JSON,
        prefix: Some("loo"),
    },
    PackageSpec {
        name: "bayesplot",
        json: BAYES_JSON,
        prefix: Some("bayesplot"),
    },
    PackageSpec {
        name: "cmdstanr",
        json: BAYES_JSON,
        prefix: Some("cmdstanr"),
    },
];

pub fn known_packages() -> impl Iterator<Item = &'static str> {
    PACKAGE_SPECS.iter().map(|spec| spec.name)
}

#[derive(Debug, Error)]
pub enum TypeshedError {
    #[error("typeshed parse error: {0}")]
    Json(#[from] serde_json::Error),
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
pub enum ReturnSlot {
    #[serde(rename = "arg0")]
    Arg0,
    #[serde(rename = "concat_of_args")]
    ConcatOfArgs,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ReturnSpec {
    /// A validated return slot whose type depends on call arguments.
    Slot(ReturnSlot),
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
        version: raw.version,
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
        version: String,
        functions: std::collections::BTreeMap<String, _fwd::_FunctionSig>,
        #[serde(default)]
        datasets: std::collections::BTreeMap<String, JsonRType>,
        #[serde(default)]
        s3_methods: Vec<RawS3Method>,
    }
    let raw: RawFile = serde_json::from_str(json)?;
    let mut functions = std::collections::BTreeMap::new();
    for (k, v) in raw.functions {
        let key = match prefix {
            Some(prefix) => {
                let qualified = format!("{prefix}.");
                let Some(name) = k.strip_prefix(&qualified) else {
                    continue;
                };
                name.to_string()
            }
            None => k,
        };
        functions.insert(
            key,
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
        version: raw.version,
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
    let _ = PACKAGE_SPECS.iter().find(|spec| spec.name == name)?;
    static PACKAGES: std::sync::OnceLock<std::collections::BTreeMap<&'static str, Typeshed>> =
        std::sync::OnceLock::new();
    let packages = PACKAGES.get_or_init(|| {
        PACKAGE_SPECS
            .iter()
            .map(|spec| {
                let typeshed = parse_package(spec.json, spec.prefix)
                    .expect("embedded package typeshed must parse");
                (spec.name, typeshed)
            })
            .collect()
    });
    packages.get(name)
}

/// Whether a package name is known to ry's embedded typeshed. Used by
/// the checker to decide whether a `library(pkg)` contributes
/// signatures (unknown packages are still recorded as loaded for NSE
/// gating, e.g. `tidyverse`, but contribute no function signatures).
pub fn is_known_package(name: &str) -> bool {
    PACKAGE_SPECS.iter().any(|spec| spec.name == name)
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
        assert!(!brms.functions.contains_key("posterior.as_draws_df"));
    }

    #[test]
    fn typeshed_preserves_embedded_schema_version() {
        let t = load_base().expect("loads");
        assert_eq!(t.version, "0.0.1");
    }

    #[test]
    fn every_known_package_loads() {
        for name in known_packages() {
            assert!(load_package(name).is_some(), "{name} must load");
        }
    }

    #[test]
    fn unknown_return_slot_is_rejected() {
        let json = r#"{"params":[],"return":"arg_0"}"#;
        assert!(serde_json::from_str::<_fwd::_FunctionSig>(json).is_err());
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
