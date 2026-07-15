//! Built-in database of base R function signatures, embedded at compile time.
//!
//! Signatures are intentionally underspecified: many use abstract slots
//! like "arg0" (type mirrors the first positional argument) or "unknown".
//! The checker resolves these slots when applying a signature.

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalMode {
    Normal,
    QuotedSymbol,
    QuotedExpression,
    DataMask,
    TidySelect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaEffect {
    Preserve,
    AddNamedArgs,
    Select,
    Aggregate,
    ExpressionValue,
    Join,
    Pivot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeEffect {
    UnknownBindings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackArg {
    ElementOfArg0,
    ElementOfArg1,
    Unknown,
    AccumulatorAndElement,
    ElementsAfterCallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HigherOrderResultKind {
    ListOfCallbackReturn,
    VectorOf,
    SameAsArg0,
    CallbackReturn,
    FirstArg,
    Simplify,
    FunValueTemplate,
    CallbackIdentity,
}

/// Checker interpretation of the free-form `mode` strings in an R type.
///
/// The JSON representation stays a string for compatibility, but both the
/// validator and checker convert it through this enum so their vocabularies
/// cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonMode {
    Arg0,
    Arg2,
    Character,
    Complex,
    Double,
    DoubleOrInt,
    Function,
    Integer,
    List,
    Logical,
    Null,
    Opaque,
    Raw,
    Union,
    YesOrNo,
}

impl JsonMode {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "arg0" => Self::Arg0,
            "arg2" => Self::Arg2,
            "character" => Self::Character,
            "complex" => Self::Complex,
            "double" => Self::Double,
            "double_or_int" => Self::DoubleOrInt,
            "function" => Self::Function,
            "integer" => Self::Integer,
            "list" => Self::List,
            "logical" => Self::Logical,
            "null" => Self::Null,
            "opaque" => Self::Opaque,
            "raw" => Self::Raw,
            "union" => Self::Union,
            "yes_or_no" => Self::YesOrNo,
            _ => return None,
        })
    }

    pub fn is_higher_order_result(self) -> bool {
        matches!(
            self,
            Self::Logical | Self::Integer | Self::Double | Self::Character | Self::Opaque
        )
    }
}

/// Checker interpretation of the free-form `length` strings in an R type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonLength {
    Known(usize),
    Arg0,
    Arg1,
    Arg2,
    LongestArg,
    NArgs,
    Test,
    Unknown,
    XTimes,
}

impl JsonLength {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "arg0" => Self::Arg0,
            "arg1" => Self::Arg1,
            "arg2" => Self::Arg2,
            "longest_arg" => Self::LongestArg,
            "n_args" => Self::NArgs,
            "test" => Self::Test,
            "unknown" => Self::Unknown,
            "x_times" => Self::XTimes,
            value => {
                const KNOWN: &[usize] = &[
                    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 15, 19, 20, 21, 24, 26, 30, 31, 32, 35,
                    39, 43, 47, 48, 49, 50, 54, 60, 64, 66, 70, 71, 72, 84, 88, 98, 100, 132, 141,
                    150, 153, 176, 240, 248, 272, 289, 468, 578, 1000, 2820,
                ];
                let parsed = value.parse().ok()?;
                if !KNOWN.contains(&parsed) {
                    return None;
                }
                Self::Known(parsed)
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HigherOrderResult {
    pub kind: HigherOrderResultKind,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub length_arg: Option<usize>,
    #[serde(default)]
    pub source_arg: Option<usize>,
    #[serde(default)]
    pub template_position: Option<usize>,
    #[serde(default)]
    pub unknown_length: bool,
    #[serde(default)]
    pub include_callback_schema: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HigherOrderSpec {
    pub callback_param: String,
    pub callback_position: usize,
    pub callback_args: Vec<CallbackArg>,
    pub result: HigherOrderResult,
}

pub const SOURCE: &str = include_str!("../vendor/SOURCE");
pub const BASE_JSON: &str = include_str!("../vendor/base/base.json");
pub const DPLYR_JSON: &str = include_str!("../vendor/dplyr/dplyr.json");
pub const DBPLYR_JSON: &str = include_str!("../vendor/dbplyr/dbplyr.json");
pub const TIDYR_JSON: &str = include_str!("../vendor/tidyr/tidyr.json");
pub const TIDYSELECT_JSON: &str = include_str!("../vendor/tidyselect/tidyselect.json");
pub const TESTTHAT_JSON: &str = include_str!("../vendor/testthat/testthat.json");
pub const TINYTEST_JSON: &str = include_str!("../vendor/tinytest/tinytest.json");
pub const RCPP_JSON: &str = include_str!("../vendor/rcpp/Rcpp.json");
pub const PURRR_JSON: &str = include_str!("../vendor/purrr/purrr.json");
pub const IGRAPH_JSON: &str = include_str!("../vendor/igraph/igraph.json");
pub const RECIPES_JSON: &str = include_str!("../vendor/recipes/recipes.json");
pub const BENCH_JSON: &str = include_str!("../vendor/bench/bench.json");
pub const BOX_JSON: &str = include_str!("../vendor/box/box.json");
pub const PATRICK_JSON: &str = include_str!("../vendor/patrick/patrick.json");
pub const REX_JSON: &str = include_str!("../vendor/rex/rex.json");
pub const RLIST_JSON: &str = include_str!("../vendor/rlist/rlist.json");
pub const MIRAI_JSON: &str = include_str!("../vendor/mirai/mirai.json");
pub const SURVIVAL_JSON: &str = include_str!("../vendor/survival/survival.json");
pub const BRMS_JSON: &str = include_str!("../vendor/brms/brms.json");
pub const POSTERIOR_JSON: &str = include_str!("../vendor/posterior/posterior.json");
pub const LOO_JSON: &str = include_str!("../vendor/loo/loo.json");
pub const BAYESPLOT_JSON: &str = include_str!("../vendor/bayesplot/bayesplot.json");
pub const CMDSTANR_JSON: &str = include_str!("../vendor/cmdstanr/cmdstanr.json");
pub const ZEALLOT_JSON: &str = include_str!("../vendor/zeallot/zeallot.json");
pub const FUTURE_JSON: &str = include_str!("../vendor/future/future.json");
pub const FOREACH_JSON: &str = include_str!("../vendor/foreach/foreach.json");
pub const SHINY_JSON: &str = include_str!("../vendor/shiny/shiny.json");
pub const WITHR_JSON: &str = include_str!("../vendor/withr/withr.json");
pub const R6_JSON: &str = include_str!("../vendor/R6/R6.json");
pub const S7_JSON: &str = include_str!("../vendor/s7/S7.json");
/// Legacy Bayesian stub document. New code should load a named package via
/// [`load_package`]; the standalone typeshed no longer publishes a combined
/// multi-package document.
#[deprecated(note = "use load_package with a Bayesian package name")]
pub const BAYES_JSON: &str = BRMS_JSON;

#[derive(Clone, Copy)]
struct PackageSpec {
    name: &'static str,
    json: &'static str,
}

/// Single source of truth for embedded non-base packages, in signature
/// resolution order. Every package maps one-to-one to its vendored file.
const PACKAGE_SPECS: &[PackageSpec] = &[
    PackageSpec {
        name: "dplyr",
        json: DPLYR_JSON,
    },
    PackageSpec {
        name: "dbplyr",
        json: DBPLYR_JSON,
    },
    PackageSpec {
        name: "tidyr",
        json: TIDYR_JSON,
    },
    PackageSpec {
        name: "tidyselect",
        json: TIDYSELECT_JSON,
    },
    PackageSpec {
        name: "purrr",
        json: PURRR_JSON,
    },
    PackageSpec {
        name: "igraph",
        json: IGRAPH_JSON,
    },
    PackageSpec {
        name: "recipes",
        json: RECIPES_JSON,
    },
    PackageSpec {
        name: "bench",
        json: BENCH_JSON,
    },
    PackageSpec {
        name: "box",
        json: BOX_JSON,
    },
    PackageSpec {
        name: "patrick",
        json: PATRICK_JSON,
    },
    PackageSpec {
        name: "rex",
        json: REX_JSON,
    },
    PackageSpec {
        name: "rlist",
        json: RLIST_JSON,
    },
    PackageSpec {
        name: "mirai",
        json: MIRAI_JSON,
    },
    PackageSpec {
        name: "survival",
        json: SURVIVAL_JSON,
    },
    PackageSpec {
        name: "testthat",
        json: TESTTHAT_JSON,
    },
    PackageSpec {
        name: "tinytest",
        json: TINYTEST_JSON,
    },
    PackageSpec {
        name: "Rcpp",
        json: RCPP_JSON,
    },
    PackageSpec {
        name: "brms",
        json: BRMS_JSON,
    },
    PackageSpec {
        name: "posterior",
        json: POSTERIOR_JSON,
    },
    PackageSpec {
        name: "loo",
        json: LOO_JSON,
    },
    PackageSpec {
        name: "bayesplot",
        json: BAYESPLOT_JSON,
    },
    PackageSpec {
        name: "cmdstanr",
        json: CMDSTANR_JSON,
    },
    PackageSpec {
        name: "zeallot",
        json: ZEALLOT_JSON,
    },
    PackageSpec {
        name: "future",
        json: FUTURE_JSON,
    },
    PackageSpec {
        name: "foreach",
        json: FOREACH_JSON,
    },
    PackageSpec {
        name: "shiny",
        json: SHINY_JSON,
    },
    PackageSpec {
        name: "withr",
        json: WITHR_JSON,
    },
    PackageSpec {
        name: "R6",
        json: R6_JSON,
    },
    PackageSpec {
        name: "S7",
        json: S7_JSON,
    },
];

pub fn known_packages() -> impl Iterator<Item = &'static str> {
    PACKAGE_SPECS.iter().map(|spec| spec.name)
}

#[derive(Debug, Error)]
pub enum TypeshedError {
    #[error("typeshed parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("failed to read typeshed `{path}`: {source}", path = path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("typeshed parse error in `{path}`: {source}", path = path.display())]
    JsonAtPath {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("unsupported typeshed schema version `{schema_version}` in `{path}`", path = path.display())]
    UnsupportedSchema {
        path: PathBuf,
        schema_version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
    /// Documentation metadata retained from the source corpus.
    #[serde(default)]
    pub note: Option<String>,
    /// Concrete member modes when `mode` is `union`.
    #[serde(default)]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParamSpec {
    pub name: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<JsonRType>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<bool>,
}

impl<'de> Deserialize<'de> for ParamSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ObjectParamSpec {
            name: String,
            #[serde(rename = "type")]
            type_: Option<JsonRType>,
            #[serde(default)]
            required: bool,
            #[serde(default)]
            default: Option<bool>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Name(String),
            Object(ObjectParamSpec),
        }

        Ok(match Repr::deserialize(deserializer)? {
            Repr::Name(name) => Self {
                name,
                type_: None,
                required: false,
                default: None,
            },
            Repr::Object(spec) => Self {
                name: spec.name,
                type_: spec.type_,
                required: spec.required,
                default: spec.default,
            },
        })
    }
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
    pub params: Vec<ParamSpec>,
    pub return_: ReturnSpec,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub eval: std::collections::BTreeMap<String, EvalMode>,
    #[serde(default)]
    pub schema_effect: Option<SchemaEffect>,
    #[serde(default)]
    pub scope_effect: Option<ScopeEffect>,
    #[serde(default)]
    pub higher_order: Option<HigherOrderSpec>,
    #[serde(default)]
    pub injects: Vec<InjectSpec>,
    /// Zero-based literal argument used as a path relative to the current
    /// source file. Consumers may fold the call only when that argument is a
    /// string literal and the target exists.
    #[serde(default)]
    pub source_relative_path_arg: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InjectSpec {
    pub into: Vec<String>,
    #[serde(default)]
    pub strings_from: Vec<String>,
    #[serde(default)]
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FunctionSigWithKey {
    pub name: String,
    #[serde(flatten)]
    pub sig: FunctionSig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Globals {
    #[serde(default)]
    pub ambient: Vec<String>,
    #[serde(default)]
    pub ambient_functions: Vec<String>,
    #[serde(default)]
    pub s3_generics: Vec<String>,
    #[serde(default)]
    pub s3_split_denylist: Vec<String>,
}

impl FunctionSig {
    pub fn params(&self) -> &[ParamSpec] {
        &self.params
    }

    pub fn param_names(&self) -> impl Iterator<Item = &str> {
        self.params.iter().map(|param| param.name.as_str())
    }

    pub fn return_(&self) -> &ReturnSpec {
        &self.return_
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Typeshed {
    /// File-format version. Absent in legacy stubs.
    #[serde(default)]
    pub schema_version: Option<String>,
    /// Package namespace supplied by standalone r-typeshed files.
    #[serde(default)]
    pub package: Option<String>,
    pub version: String,
    pub functions: std::collections::BTreeMap<String, FunctionSig>,
    /// Optional checker-wide name and S3 metadata. This normally comes
    /// from the base stub and is replaced with it by runtime overrides.
    #[serde(default)]
    pub globals: Globals,
    /// Built-in datasets (`mtcars`, `iris`, ...) typed as list-typed
    /// values. These resolve in the checker when an identifier is not
    /// bound by user code or function scope.
    #[serde(default)]
    pub datasets: std::collections::BTreeMap<String, JsonRType>,
    /// Built-in S3 methods keyed by `(generic, class)`. The checker
    /// consults this during S3 dispatch; a `default` entry is a valid
    /// fallback for any class without a more specific method.
    #[serde(default)]
    pub s3_methods: std::collections::BTreeMap<(String, String), FunctionSig>,
}

/// Wrapper to handle the JSON shape where the key "return" is reserved
/// (it's a Rust keyword). We deserialize via a serde alias.
mod _fwd {
    use serde::{Deserialize, Serialize};
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    pub struct _FunctionSig {
        pub params: Vec<super::ParamSpec>,
        #[serde(rename = "return")]
        pub return_: super::ReturnSpec,
        #[serde(default)]
        pub aliases: Vec<String>,
        #[serde(default)]
        pub eval: std::collections::BTreeMap<String, super::EvalMode>,
        #[serde(default)]
        pub schema_effect: Option<super::SchemaEffect>,
        #[serde(default)]
        pub scope_effect: Option<super::ScopeEffect>,
        #[serde(default)]
        pub higher_order: Option<super::HigherOrderSpec>,
        #[serde(default)]
        pub injects: Vec<super::InjectSpec>,
        #[serde(default)]
        pub source_relative_path_arg: Option<usize>,
    }
}

/// JSON shape for a single S3 method entry in `base_r.json`. The
/// `(generic, class)` pair becomes the BTreeMap key after deserialization.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawS3Method {
    generic: String,
    class: String,
    params: Vec<ParamSpec>,
    #[serde(rename = "return")]
    return_: ReturnSpec,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    eval: BTreeMap<String, EvalMode>,
    #[serde(default)]
    schema_effect: Option<SchemaEffect>,
    #[serde(default)]
    scope_effect: Option<ScopeEffect>,
    #[serde(default)]
    higher_order: Option<HigherOrderSpec>,
    #[serde(default)]
    injects: Vec<InjectSpec>,
    #[serde(default)]
    source_relative_path_arg: Option<usize>,
}

pub fn load_base() -> Result<Typeshed, TypeshedError> {
    parse_typeshed(BASE_JSON, Path::new("<embedded base>"))
}

struct RawFunctions(Vec<(String, _fwd::_FunctionSig)>);

impl<'de> Deserialize<'de> for RawFunctions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RawFunctionsVisitor;

        impl<'de> Visitor<'de> for RawFunctionsVisitor {
            type Value = RawFunctions;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object mapping function names to signatures")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut entries = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(RawFunctions(entries))
            }
        }

        deserializer.deserialize_map(RawFunctionsVisitor)
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    schema_version: Option<String>,
    #[serde(default)]
    package: Option<String>,
    version: String,
    functions: RawFunctions,
    #[serde(default)]
    globals: Globals,
    #[serde(default)]
    datasets: BTreeMap<String, JsonRType>,
    #[serde(default)]
    s3_methods: Vec<RawS3Method>,
}

fn parse_typeshed_with_order(
    json: &str,
    path: &Path,
) -> Result<(Typeshed, Vec<String>), TypeshedError> {
    let raw: RawFile = serde_json::from_str(json).map_err(|source| TypeshedError::JsonAtPath {
        path: path.to_path_buf(),
        source,
    })?;
    if let Some(schema_version) = raw.schema_version.as_deref()
        && schema_version != "1"
    {
        return Err(TypeshedError::UnsupportedSchema {
            path: path.to_path_buf(),
            schema_version: schema_version.to_string(),
        });
    }
    let mut functions = std::collections::BTreeMap::new();
    let function_order = raw
        .functions
        .0
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    for (k, v) in raw.functions.0 {
        functions.insert(
            k,
            FunctionSig {
                params: v.params,
                return_: v.return_,
                aliases: v.aliases,
                eval: v.eval,
                schema_effect: v.schema_effect,
                scope_effect: v.scope_effect,
                higher_order: v.higher_order,
                injects: v.injects,
                source_relative_path_arg: v.source_relative_path_arg,
            },
        );
    }
    let mut s3_methods = std::collections::BTreeMap::new();
    for m in raw.s3_methods {
        let key = (m.generic, m.class);
        s3_methods.insert(
            key,
            FunctionSig {
                params: m.params,
                return_: m.return_,
                aliases: m.aliases,
                eval: m.eval,
                schema_effect: m.schema_effect,
                scope_effect: m.scope_effect,
                higher_order: m.higher_order,
                injects: m.injects,
                source_relative_path_arg: m.source_relative_path_arg,
            },
        );
    }
    Ok((
        Typeshed {
            schema_version: raw.schema_version,
            package: raw.package,
            version: raw.version,
            functions,
            globals: raw.globals,
            datasets: raw.datasets,
            s3_methods,
        },
        function_order,
    ))
}

fn parse_typeshed(json: &str, path: &Path) -> Result<Typeshed, TypeshedError> {
    parse_typeshed_with_order(json, path).map(|(typeshed, _)| typeshed)
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

/// Load a non-base package's typeshed. Returns `None` for an unknown
/// package name (the checker treats that as "no signatures available",
/// i.e. opaque). Known packages and their JSON sources:
///
/// - `dplyr` -> `data/dplyr.json` (bare function names).
/// - `purrr` -> `data/purrr.json` (bare function names).
/// - `mirai` -> `data/mirai.json` (bare function names).
/// - `survival` -> `data/survival.json` (bare function names).
/// - `testthat` -> `data/testthat.json` (bare function names).
/// - `brms`, `posterior`, `loo`, `bayesplot`, `cmdstanr` each map to a
///   separate vendored package file with bare function names.
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
                let typeshed = parse_typeshed(spec.json, Path::new(spec.name))
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

/// Load stub files from a user-supplied directory. Both flat
/// (`<dir>/<pkg>.json`) and nested (`<dir>/<pkg>/<pkg>.json`) layouts are
/// accepted. The `package` header names the package; legacy files fall back
/// to their file stem. A user package replaces an embedded package wholesale.
pub fn load_stub_dir(dir: &Path) -> Result<BTreeMap<String, Typeshed>, TypeshedError> {
    let (stubs, errors) = load_stub_dir_with_warnings(dir)?;
    if let Some(error) = errors.into_iter().next() {
        return Err(error);
    }
    Ok(stubs)
}

/// Tolerant variant used by long-running and batch frontends. Valid files are
/// retained while each malformed file is returned for warning emission.
pub fn load_stub_dir_with_warnings(
    dir: &Path,
) -> Result<(BTreeMap<String, Typeshed>, Vec<TypeshedError>), TypeshedError> {
    let paths = discover_stub_files(dir)?;

    let mut stubs = BTreeMap::new();
    let mut errors = Vec::new();
    for path in paths {
        match load_stub_file(&path) {
            Ok(typeshed) => {
                let package = typeshed.package.clone().or_else(|| {
                    path.file_stem()
                        .and_then(|stem| stem.to_str())
                        .map(str::to_owned)
                });
                if let Some(package) = package {
                    stubs.insert(package, typeshed);
                }
            }
            Err(error) => errors.push(error),
        }
    }
    Ok((stubs, errors))
}

/// Discover the files accepted by [`load_stub_dir`]. This is public so tools
/// such as `ry typeshed validate` use exactly the runtime loader's layouts.
pub fn discover_stub_files(dir: &Path) -> Result<Vec<PathBuf>, TypeshedError> {
    let entries = std::fs::read_dir(dir).map_err(|source| TypeshedError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| TypeshedError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            paths.push(path);
            continue;
        }
        if path.is_dir() {
            let Some(name) = path.file_name() else {
                continue;
            };
            let nested = path.join(name).with_extension("json");
            if nested.is_file() {
                paths.push(nested);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

/// Load one stub through the normative parser used by the runtime loader.
pub fn load_stub_file(path: &Path) -> Result<Typeshed, TypeshedError> {
    load_stub_file_with_order(path).map(|(typeshed, _)| typeshed)
}

fn load_stub_file_with_order(path: &Path) -> Result<(Typeshed, Vec<String>), TypeshedError> {
    let json = std::fs::read_to_string(path).map_err(|source| TypeshedError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_typeshed_with_order(&json, path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationLevel {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationProblem {
    pub path: PathBuf,
    pub level: ValidationLevel,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct ValidationReport {
    pub files: usize,
    pub problems: Vec<ValidationProblem>,
}

impl ValidationReport {
    pub fn error_count(&self) -> usize {
        self.problems
            .iter()
            .filter(|problem| problem.level == ValidationLevel::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.problems
            .iter()
            .filter(|problem| problem.level == ValidationLevel::Warning)
            .count()
    }
}

/// Validate repository conventions on top of the normative runtime parser.
pub fn validate_stub_dirs(dirs: &[PathBuf]) -> ValidationReport {
    let mut report = ValidationReport::default();
    for dir in dirs {
        let paths = match discover_stub_files(dir) {
            Ok(paths) => paths,
            Err(error) => {
                report.problems.push(ValidationProblem {
                    path: dir.clone(),
                    level: ValidationLevel::Error,
                    message: validation_error_message(&error),
                });
                continue;
            }
        };
        if paths.is_empty() {
            report.problems.push(ValidationProblem {
                path: dir.clone(),
                level: ValidationLevel::Error,
                message: "no stub files found".to_string(),
            });
            continue;
        }
        for path in paths {
            report.files += 1;
            validate_stub_file(&path, &mut report);
        }
    }
    report
}

fn validation_error_message(error: &TypeshedError) -> String {
    match error {
        TypeshedError::Json(source) => format!("typeshed parse error: {source}"),
        TypeshedError::Io { source, .. } => format!("failed to read typeshed: {source}"),
        TypeshedError::JsonAtPath { source, .. } => format!("typeshed parse error: {source}"),
        TypeshedError::UnsupportedSchema { schema_version, .. } => {
            format!("unsupported typeshed schema version `{schema_version}`")
        }
    }
}

fn validate_stub_file(path: &Path, report: &mut ValidationReport) {
    let (typeshed, function_order) = match load_stub_file_with_order(path) {
        Ok(parsed) => parsed,
        Err(error) => {
            report.problems.push(ValidationProblem {
                path: path.to_path_buf(),
                level: ValidationLevel::Error,
                message: validation_error_message(&error),
            });
            return;
        }
    };

    if typeshed.schema_version.is_none() {
        validation_error(report, path, "missing required `schema_version` field");
    }
    let expected_package = path.file_stem().and_then(|stem| stem.to_str());
    match (typeshed.package.as_deref(), expected_package) {
        (None, _) => validation_error(report, path, "missing required `package` field"),
        (Some(actual), Some(expected)) if actual != expected => validation_error(
            report,
            path,
            format!("package `{actual}` does not match file name `{expected}.json`"),
        ),
        _ => {}
    }

    if !function_order.windows(2).all(|pair| pair[0] <= pair[1]) {
        report.problems.push(ValidationProblem {
            path: path.to_path_buf(),
            level: ValidationLevel::Warning,
            message: "function keys are not sorted".to_string(),
        });
    }

    let mut owners: HashMap<&str, &str> = typeshed
        .functions
        .keys()
        .map(|name| (name.as_str(), name.as_str()))
        .collect();
    for (name, signature) in &typeshed.functions {
        validate_signature(report, path, &format!("functions.{name}"), signature);
        for alias in &signature.aliases {
            if let Some(owner) = owners.insert(alias, name) {
                validation_error(
                    report,
                    path,
                    format!(
                        "duplicate function name `{alias}` after alias expansion (`{owner}` and `{name}`)"
                    ),
                );
            }
        }
    }
    for (name, rtype) in &typeshed.datasets {
        validate_rtype(report, path, &format!("datasets.{name}"), rtype);
    }
    for ((generic, class), signature) in &typeshed.s3_methods {
        validate_signature(
            report,
            path,
            &format!("s3_methods[{generic}.{class}]"),
            signature,
        );
    }
}

fn validation_error(report: &mut ValidationReport, path: &Path, message: impl Into<String>) {
    report.problems.push(ValidationProblem {
        path: path.to_path_buf(),
        level: ValidationLevel::Error,
        message: message.into(),
    });
}

fn validate_signature(
    report: &mut ValidationReport,
    path: &Path,
    location: &str,
    signature: &FunctionSig,
) {
    for (index, param) in signature.params.iter().enumerate() {
        let param_location = format!("{location}.params[{index}]");
        if param.name.is_empty() {
            validation_error(
                report,
                path,
                format!("{param_location}.name: must not be empty"),
            );
        }
        if param.name == "..." && (param.required || param.type_.is_some()) {
            validation_error(
                report,
                path,
                format!("{param_location}: `...` cannot be required or typed"),
            );
        }
        if param.required && param.default == Some(true) {
            validation_error(
                report,
                path,
                format!("{param_location}: a required parameter cannot have a default"),
            );
        }
        if let Some(rtype) = &param.type_ {
            validate_rtype(report, path, &format!("{param_location}.type"), rtype);
        }
    }
    if let ReturnSpec::Concrete(rtype) = &signature.return_ {
        validate_rtype(report, path, &format!("{location}.return"), rtype);
    }
    if let Some(higher_order) = &signature.higher_order
        && let Some(mode) = higher_order.result.mode.as_deref()
        && !JsonMode::parse(mode).is_some_and(JsonMode::is_higher_order_result)
    {
        validation_error(
            report,
            path,
            format!("{location}.higher_order.result.mode: invalid mode `{mode}`"),
        );
    }
}

fn validate_rtype(report: &mut ValidationReport, path: &Path, location: &str, rtype: &JsonRType) {
    if JsonMode::parse(&rtype.mode).is_none() {
        validation_error(
            report,
            path,
            format!("{location}.mode: invalid mode `{}`", rtype.mode),
        );
    }
    if JsonLength::parse(&rtype.length).is_none() {
        validation_error(
            report,
            path,
            format!("{location}.length: invalid length `{}`", rtype.length),
        );
    }
    if rtype.mode == "union" {
        if rtype.members.is_empty() {
            validation_error(
                report,
                path,
                format!("{location}.members: union must not be empty"),
            );
        }
        for member in &rtype.members {
            if !matches!(
                JsonMode::parse(member),
                Some(
                    JsonMode::Character
                        | JsonMode::Complex
                        | JsonMode::Double
                        | JsonMode::Function
                        | JsonMode::Integer
                        | JsonMode::List
                        | JsonMode::Logical
                        | JsonMode::Null
                        | JsonMode::Opaque
                        | JsonMode::Raw
                )
            ) {
                validation_error(
                    report,
                    path,
                    format!("{location}.members: invalid concrete mode `{member}`"),
                );
            }
        }
    } else if !rtype.members.is_empty() {
        validation_error(
            report,
            path,
            format!("{location}.members: only `union` mode may declare members"),
        );
    }
    for (name, column) in &rtype.columns {
        validate_rtype(report, path, &format!("{location}.columns.{name}"), column);
    }
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
    fn path_constructor_semantics_are_declarative() {
        let testthat = load_package("testthat").expect("testthat is known");
        assert_eq!(
            testthat.functions["test_path"].source_relative_path_arg,
            Some(0)
        );
    }

    #[test]
    fn scope_effect_is_loaded_from_function_stub() {
        let typeshed = parse_typeshed(
            r#"{"schema_version":"1","package":"effects","version":"test","functions":{"inject":{"params":[],"return":{"mode":"opaque","length":"unknown"},"scope_effect":"unknown_bindings"}}}"#,
            Path::new("effects.json"),
        )
        .expect("scope effect stub loads");
        assert_eq!(
            typeshed.functions["inject"].scope_effect,
            Some(ScopeEffect::UnknownBindings)
        );
    }

    #[test]
    fn load_package_bayes_files_have_bare_names() {
        let brms_stub = load_package("brms").expect("brms is known");
        assert!(brms_stub.functions.contains_key("brm"));
        assert!(brms_stub.functions.contains_key("posterior_predict"));

        let posterior = load_package("posterior").expect("posterior is known");
        assert!(posterior.functions.contains_key("as_draws_df"));
        assert!(!posterior.functions.contains_key("posterior.as_draws_df"));
        // The brms-only view must NOT see posterior's entries.
        assert!(!brms_stub.functions.contains_key("as_draws_df"));
        assert!(!brms_stub.functions.contains_key("posterior.as_draws_df"));
        assert_eq!(
            posterior.functions["mutate_variables"].eval.get("..."),
            Some(&EvalMode::DataMask)
        );
    }

    fn fixture(package: Option<&str>, function: &str) -> String {
        let package = package
            .map(|name| format!(r#", "package": "{name}""#))
            .unwrap_or_default();
        format!(
            r#"{{"schema_version":"1"{package},"version":"test","functions":{{"{function}":{{"params":[],"return":{{"mode":"integer","length":"1"}}}}}}}}"#
        )
    }

    #[test]
    fn load_stub_dir_accepts_flat_and_nested_layouts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("flat.json"), fixture(Some("flat"), "one")).unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(
            dir.path().join("nested/nested.json"),
            fixture(Some("nested"), "two"),
        )
        .unwrap();

        let stubs = load_stub_dir(dir.path()).unwrap();
        assert!(stubs["flat"].functions.contains_key("one"));
        assert!(stubs["nested"].functions.contains_key("two"));
    }

    #[test]
    fn load_stub_dir_uses_file_stem_without_package_header() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("legacy.json"), fixture(None, "old")).unwrap();
        let stubs = load_stub_dir(dir.path()).unwrap();
        assert!(stubs["legacy"].functions.contains_key("old"));
    }

    #[test]
    fn user_stub_wholesale_overrides_embedded_package_and_base() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("dplyr.json"),
            fixture(Some("dplyr"), "custom"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("base.json"),
            fixture(Some("base"), "custom_base"),
        )
        .unwrap();
        let stubs = load_stub_dir(dir.path()).unwrap();
        assert!(stubs["dplyr"].functions.contains_key("custom"));
        assert!(!stubs["dplyr"].functions.contains_key("filter"));
        assert!(stubs["base"].functions.contains_key("custom_base"));
        assert!(!stubs["base"].functions.contains_key("length"));
    }

    #[test]
    fn malformed_stub_error_names_the_file_and_lossy_load_keeps_valid_files() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "not json").unwrap();
        std::fs::write(dir.path().join("good.json"), fixture(Some("good"), "ok")).unwrap();

        let error = load_stub_dir(dir.path()).unwrap_err();
        assert!(error.to_string().contains(&bad.display().to_string()));
        let (stubs, errors) = load_stub_dir_with_warnings(dir.path()).unwrap();
        assert!(stubs.contains_key("good"));
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn unsupported_schema_is_rejected_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.json");
        let json = fixture(Some("future"), "f")
            .replace(r#""schema_version":"1""#, r#""schema_version":"2""#);
        std::fs::write(&path, json).unwrap();
        let error = load_stub_dir(dir.path()).unwrap_err();
        assert!(matches!(error, TypeshedError::UnsupportedSchema { .. }));
        assert!(error.to_string().contains(&path.display().to_string()));
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
    fn params_accept_legacy_names_and_typed_objects() {
        let json = r#"{"params":["x",{"name":"trim","type":{"mode":"logical","length":"1"},"required":true,"default":false}],"return":"arg0"}"#;
        let signature: _fwd::_FunctionSig = serde_json::from_str(json).unwrap();
        assert_eq!(signature.params[0].name, "x");
        assert!(!signature.params[0].required);
        assert_eq!(signature.params[1].name, "trim");
        assert_eq!(signature.params[1].type_.as_ref().unwrap().mode, "logical");
        assert!(signature.params[1].required);
        assert_eq!(signature.params[1].default, Some(false));
    }

    #[test]
    fn function_injects_are_parsed() {
        let json = r#"{"params":["new","code"],"injects":[{"into":["code"],"strings_from":["new"],"names":["self"]}],"return":{"mode":"opaque","length":"unknown"}}"#;
        let signature: _fwd::_FunctionSig = serde_json::from_str(json).unwrap();
        assert_eq!(signature.injects.len(), 1);
        assert_eq!(signature.injects[0].into, ["code"]);
        assert_eq!(signature.injects[0].strings_from, ["new"]);
        assert_eq!(signature.injects[0].names, ["self"]);
    }

    #[test]
    fn param_objects_reject_unknown_fields() {
        let json = r#"{"params":[{"name":"x","optional":true}],"return":"arg0"}"#;
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
