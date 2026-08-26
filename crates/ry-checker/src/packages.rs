//! Static R package metadata — re-exported from ry-workspace.
//!
//! The implementation lives in `ry-workspace` so that `ry-workspace`
//! does not depend on `ry-checker`. This module preserves the
//! `ry_checker::packages::*` import path for backward compatibility.

pub use ry_workspace::packages::{
    NATIVE_REGISTRATION_SENTINEL, NATIVE_ROUTINE_PREFIX_SENTINEL, NamespaceMetadata,
    attached_packages, namespace_metadata,
};
