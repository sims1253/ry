//! Project function definitions: which collected literal a spelling denotes.
//!
//! A definition's identity is its source path and literal span, independent of
//! the names that bind it. Each raw binding spelling names its last definition
//! (later bindings win, as R's top-level rebinding does). A literal that a
//! later binding replaced stays addressable by identity for declaration checks.

use crate::infer::semantic_argument_name;
use crate::{FxMap, FxSet, UserFn};
use ry_core::Span;

mod s4;
pub(crate) use s4::{GenericDecl, S4Call, S4Inventory, S4Selection};

/// Source identity of one function literal.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FunctionId {
    path: String,
    start: usize,
    end: usize,
}

impl FunctionId {
    pub(crate) fn new(path: &str, span: Span) -> Self {
        Self {
            path: path.to_string(),
            start: span.start,
            end: span.end,
        }
    }
}

impl UserFn {
    pub(crate) fn id(&self) -> FunctionId {
        FunctionId::new(&self.source_path, self.definition_span)
    }
}

/// A function-position spelling, parsed once. `%op%` is an infix name even
/// when it contains `::`. The member keeps its raw spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Callee<'a> {
    Bare(&'a str),
    /// `pkg::member` and `pkg:::member`.
    Qualified {
        package: &'a str,
        member: &'a str,
    },
}

impl<'a> Callee<'a> {
    pub(crate) fn parse(spelling: &'a str) -> Self {
        if spelling.starts_with('%') && spelling.ends_with('%') {
            return Self::Bare(spelling);
        }
        match crate::resolve::split_qualified(spelling) {
            Some((package, member)) => Self::Qualified { package, member },
            None => Self::Bare(spelling),
        }
    }

    pub(crate) fn member(self) -> &'a str {
        match self {
            Self::Bare(member) | Self::Qualified { member, .. } => member,
        }
    }

    pub(crate) fn is_qualified(self) -> bool {
        matches!(self, Self::Qualified { .. })
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Definitions {
    /// The last definition bound to each raw spelling.
    live: FxMap<String, UserFn>,
    /// The raw spelling whose live entry is each definition.
    live_ids: FxMap<FunctionId, String>,
    /// Literals a later same-spelling binding replaced.
    replaced: FxMap<FunctionId, UserFn>,
    /// Names whose live entry need not be the function a call reaches:
    /// literals with disagreeing formals (which one wins depends on
    /// evaluation and collation order), and names a global non-literal or
    /// outward write may rebind.
    unstable: FxSet<String>,
    /// Names more than one literal binds.
    redefined: FxSet<String>,
    /// Every raw spelling's prefixes that end before a `.` (`print` for
    /// `print.foo`), so an S3 method lookup does not scan `live` (#697).
    method_prefixes: FxSet<String>,
    /// S4 generic and method registrations; methods name their literals
    /// by raw binding spelling in `live`.
    s4: S4Inventory,
}

impl Definitions {
    /// The definition a call head or callback with this spelling selects. A
    /// qualified spelling selects only a definition from that package's R
    /// sources. Unknown provenance cannot establish namespace membership. An
    /// exact raw binding wins, else any quoted binding of the same name.
    pub(crate) fn select(&self, callee: Callee<'_>) -> Option<&UserFn> {
        self.select_in(callee, None)
    }

    /// [`Self::select`] for a call made from `namespace`'s `R/` code. R
    /// builds a namespace from `R/` only, so a bare spelling there selects
    /// only that package's definitions, never a test's or script's.
    pub(crate) fn select_in(&self, callee: Callee<'_>, namespace: Option<&str>) -> Option<&UserFn> {
        let member = callee.member();
        self.live
            .get(member)
            .or_else(|| {
                // Only a quoted spelling can still match. Probe those
                // directly; scanning `live` made every unresolved call
                // proportional to the number of definitions (#697). When
                // several match, keep the scan's choice.
                let mut quoted = ['`', '\'', '"']
                    .into_iter()
                    .filter_map(|quote| self.live.get(&format!("{quote}{member}{quote}")));
                let first = quoted.next()?;
                if quoted.next().is_none() {
                    return Some(first);
                }
                self.live
                    .iter()
                    .find(|(name, _)| semantic_argument_name(name) == member)
                    .map(|(_, function)| function)
            })
            .filter(|function| {
                let package = match callee {
                    Callee::Bare(_) => namespace,
                    Callee::Qualified { package, .. } => Some(package),
                };
                package.is_none_or(|package| function.source_package.as_deref() == Some(package))
            })
    }

    /// Conservative guard: whether a project binding has this exact raw
    /// spelling. A quoted binding does not satisfy its bare spelling.
    pub(crate) fn binds(&self, raw: &str) -> bool {
        self.live.contains_key(raw)
    }

    /// Whether some raw spelling starts with `generic` followed by `.`.
    pub(crate) fn has_method_prefix(&self, generic: &str) -> bool {
        self.method_prefixes.contains(generic)
    }

    /// Any recorded direct literal, live or replaced, by source identity.
    pub(crate) fn get(&self, id: &FunctionId) -> Option<&UserFn> {
        match self.live_ids.get(id) {
            Some(name) => self.live.get(name),
            None => self.replaced.get(id),
        }
    }

    /// Whether the literal is still the last definition of some spelling.
    pub(crate) fn is_live(&self, id: &FunctionId) -> bool {
        self.live_ids.contains_key(id)
    }

    /// Whether every binding of `name` is a literal with the same formals.
    pub(crate) fn has_single_contract(&self, name: &str) -> bool {
        !self.unstable.contains(semantic_argument_name(name))
    }

    /// Whether `name` has a single contract and only one literal binds it.
    pub(crate) fn has_single_definition(&self, name: &str) -> bool {
        self.has_single_contract(name) && !self.redefined.contains(semantic_argument_name(name))
    }

    /// Note that a non-literal or outward write may rebind `name`.
    pub(crate) fn mark_unstable(&mut self, name: String) {
        self.unstable.insert(name);
    }

    // Name-keyed storage for the fixpoint and incremental `Project`, whose
    // refinement scope is keyed by raw binding spelling.

    /// The live definition of every raw binding spelling.
    pub(crate) fn live(&self) -> impl Iterator<Item = (&String, &UserFn)> {
        self.live.iter()
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = &String> {
        self.live.keys()
    }

    pub(crate) fn live_iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut UserFn)> {
        self.live.iter_mut()
    }

    pub(crate) fn live_named(&self, raw: &str) -> Option<&UserFn> {
        self.live.get(raw)
    }

    pub(crate) fn live_mut(&mut self, raw: &str) -> Option<&mut UserFn> {
        self.live.get_mut(raw)
    }

    pub(crate) fn len(&self) -> usize {
        self.live.len()
    }

    /// Bind `raw` to `function`, keeping a replaced literal addressable.
    /// Returns the replaced entry's return slot.
    pub(crate) fn bind(&mut self, raw: String, function: UserFn) -> Option<usize> {
        let id = function.id();
        let previous = self.bind_live(raw.clone(), function);
        self.live_ids.insert(id, raw);
        previous
    }

    /// Bind without recording which spelling names the new literal.
    fn bind_live(&mut self, raw: String, function: UserFn) -> Option<usize> {
        let id = function.id();
        // Quoted spellings (`` `f` ``, `"f"`) bind the same R name as `f`.
        let semantic = semantic_argument_name(&raw).to_string();
        let others: Vec<&UserFn> = [
            semantic.clone(),
            format!("`{semantic}`"),
            format!("'{semantic}'"),
            format!("\"{semantic}\""),
        ]
        .iter()
        .filter_map(|spelling| self.live.get(spelling))
        .filter(|other| other.id() != id)
        .collect();
        if others.iter().any(|other| !other.formals_agree(&function)) {
            self.unstable.insert(semantic.clone());
        }
        if !others.is_empty() {
            self.redefined.insert(semantic);
        }
        for (dot, _) in raw.match_indices('.') {
            self.method_prefixes.insert(raw[..dot].to_string());
        }
        let previous = self.live.insert(raw.clone(), function)?;
        let previous_id = previous.id();
        // Another spelling may still bind the same literal (`f <- g <- function() ...`).
        if previous_id != id && self.live_ids.get(&previous_id) == Some(&raw) {
            self.live_ids.remove(&previous_id);
            let slot = previous.return_slot;
            self.replaced.insert(previous_id, previous);
            return Some(slot);
        }
        Some(previous.return_slot)
    }

    /// Merge a later file's definitions. Its bindings replace same-spelled
    /// earlier ones. `slot_offset` shifts its return slots into this table's.
    pub(crate) fn append(&mut self, later: &Definitions, slot_offset: usize) {
        let shifted = |function: &UserFn| {
            let mut function = function.clone();
            function.return_slot += slot_offset;
            function
        };
        for (name, function) in &later.live {
            self.bind_live(name.clone(), shifted(function));
        }
        // The later table already decided which spelling names a literal
        // bound twice (`f <- g <- function() ...`); keep its decision rather
        // than the merge's iteration order.
        self.live_ids.extend(
            later
                .live_ids
                .iter()
                .map(|(id, name)| (id.clone(), name.clone())),
        );
        self.replaced.extend(
            later
                .replaced
                .iter()
                .map(|(id, function)| (id.clone(), shifted(function))),
        );
        self.unstable.extend(later.unstable.iter().cloned());
        self.redefined.extend(later.redefined.iter().cloned());
        self.s4.append(&later.s4);
    }
}

/// The live definition of a raw binding spelling; panics when unbound.
impl<S: AsRef<str> + ?Sized> std::ops::Index<&S> for Definitions {
    type Output = UserFn;

    fn index(&self, raw: &S) -> &UserFn {
        &self.live[raw.as_ref()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UserParam;
    use ry_core::types::RType;
    use std::sync::{Arc, OnceLock};

    fn literal(path: &str, start: usize, formals: &[&str], package: Option<&str>) -> UserFn {
        UserFn {
            params: formals
                .iter()
                .map(|name| UserParam {
                    name: name.to_string(),
                    type_: RType::unknown(),
                    required: true,
                    defaulted: false,
                    defused: false,
                    forcing_defaults: Vec::new(),
                    quoting: false,
                    unforced: false,
                    missing_only: false,
                    injection: None,
                })
                .collect(),
            source_params: Arc::from(Vec::new()),
            source_path: path.to_string(),
            source_package: package.map(str::to_string),
            definition_span: Span {
                start,
                end: start + 10,
                ..Span::default()
            },
            source_native_path: None,
            body: Arc::from(Vec::new()),
            outward_writes: Arc::new(OnceLock::new()),
            contract: None,
            return_slot: start,
        }
    }

    #[test]
    fn callee_parse_splits_qualifiers_but_not_infix_names() {
        assert_eq!(Callee::parse("f"), Callee::Bare("f"));
        assert_eq!(Callee::parse("%::%"), Callee::Bare("%::%"));
        assert_eq!(
            Callee::parse("pkg::f"),
            Callee::Qualified {
                package: "pkg",
                member: "f"
            }
        );
        assert_eq!(
            Callee::parse("pkg:::f"),
            Callee::Qualified {
                package: "pkg",
                member: "f"
            }
        );
    }

    #[test]
    fn select_requires_the_qualifying_package_to_own_the_definition() {
        let mut definitions = Definitions::default();
        definitions.bind("f".into(), literal("R/a.R", 1, &["x"], Some("mypkg")));
        definitions.bind("g".into(), literal("a.R", 20, &["x"], None));
        let selected = |spelling| definitions.select(Callee::parse(spelling)).is_some();
        assert!(selected("f") && selected("mypkg::f") && selected("mypkg:::f"));
        assert!(!selected("other::f"));
        // Unknown provenance cannot establish namespace membership.
        assert!(selected("g") && !selected("pkg::g"));
    }

    #[test]
    fn exact_raw_binding_wins_over_a_later_quoted_one() {
        let mut definitions = Definitions::default();
        definitions.bind("f".into(), literal("a.R", 1, &["x"], None));
        definitions.bind("`f`".into(), literal("a.R", 20, &["y"], None));
        definitions.bind("`%+%`".into(), literal("a.R", 40, &["e1", "e2"], None));
        let select = |spelling| {
            definitions
                .select(Callee::Bare(spelling))
                .unwrap()
                .return_slot
        };
        assert_eq!(select("f"), 1);
        assert_eq!(select("%+%"), 40);
        assert!(definitions.binds("f") && !definitions.binds("%+%"));
        assert!(!definitions.has_single_contract("f"));
    }

    #[test]
    fn a_second_literal_for_a_name_ends_its_single_definition() {
        let mut definitions = Definitions::default();
        let shared = literal("a.R", 1, &["x"], None);
        definitions.bind("f".into(), shared.clone());
        definitions.bind("g".into(), shared);
        assert!(definitions.has_single_definition("f"));
        let mut later = Definitions::default();
        later.bind("`f`".into(), literal("b.R", 1, &["x"], None));
        definitions.append(&later, 0);
        assert!(!definitions.has_single_definition("f"));
        assert!(definitions.has_single_contract("f"));
        assert!(definitions.has_single_definition("g"));
    }

    #[test]
    fn replaced_literal_stays_addressable_by_identity() {
        let mut definitions = Definitions::default();
        let first = literal("a.R", 1, &["x"], None);
        let shared = literal("a.R", 20, &["x"], None);
        assert_eq!(definitions.bind("f".into(), first.clone()), None);
        assert_eq!(definitions.bind("f".into(), shared.clone()), Some(1));
        // `f <- g <- function(x) ...`: rebinding `f` leaves the literal live as `g`.
        definitions.bind("g".into(), shared.clone());
        definitions.bind("f".into(), literal("a.R", 40, &["x"], None));
        assert!(!definitions.is_live(&first.id()));
        assert_eq!(definitions.get(&first.id()).unwrap().return_slot, 1);
        assert!(definitions.is_live(&shared.id()));
        assert_eq!(definitions["f"].return_slot, 40);
        assert!(definitions.has_single_contract("f"));
    }

    #[test]
    fn append_keeps_the_later_tables_identity_winners() {
        let mut earlier = Definitions::default();
        earlier.bind("f".into(), literal("a.R", 1, &["x"], None));
        let mut later = Definitions::default();
        // One literal bound twice; `g` is the spelling the later file bound last.
        let mut as_f = literal("b.R", 1, &["y"], None);
        as_f.return_slot = 0;
        let mut as_g = as_f.clone();
        as_g.return_slot = 1;
        let id = as_f.id();
        later.bind("f".into(), as_f);
        later.bind("g".into(), as_g);
        earlier.append(&later, 100);
        assert_eq!(earlier.get(&id).unwrap().return_slot, 101);
        assert_eq!(earlier["f"].return_slot, 100);
        let replaced = literal("a.R", 1, &["x"], None).id();
        assert!(!earlier.is_live(&replaced) && earlier.get(&replaced).is_some());
        // The two files disagree on `f`'s formals.
        assert!(!earlier.has_single_contract("f"));
    }
}
