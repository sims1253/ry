//! Mutation-proportional branch snapshots and rollback.
use super::*;
use crate::reference_facts::{BindingProvenance, ReferenceBlocker, ScopeProvenance};

#[derive(Debug, Clone, Default)]
pub(crate) struct BindingState {
    pub ty: Option<RType>,
    pub narrowed: bool,
    pub parameter: bool,
    pub list_origin: bool,
    pub default_parameter: bool,
    lexical: bool,
    alias: Option<String>,
    provenance: Option<BindingProvenance>,
}

impl BindingState {
    pub fn view(&self) -> BindingView<'_> {
        BindingView {
            ty: self.ty.as_ref(),
            narrowed: self.narrowed,
            list_origin: self.list_origin,
            default_parameter: self.default_parameter,
        }
    }

    fn capture(scope: &Scope, name: &str) -> Self {
        let mut state = Self::capture_metadata(scope, name);
        state.ty = scope.get(name).cloned();
        state
    }

    fn capture_metadata(scope: &Scope, name: &str) -> Self {
        Self {
            ty: None,
            narrowed: scope.narrowed_bindings.contains(name),
            parameter: scope.parameter_bindings.contains(name),
            list_origin: scope.list_origin_bindings.contains(name),
            default_parameter: scope.default_parameter_bindings.contains(name),
            lexical: scope.lexical_functions.contains(name),
            alias: scope.function_aliases.get(name).cloned(),
            provenance: scope
                .reference_provenance
                .as_ref()
                .and_then(|p| p.bindings.get(name).cloned()),
        }
    }
    fn restore(self, scope: &mut Scope, name: String) {
        fn marker(set: &mut FxSet<String>, name: &str, present: bool) {
            if present {
                set.insert(name.to_string());
            } else {
                set.remove(name);
            }
        }
        marker(&mut scope.narrowed_bindings, &name, self.narrowed);
        marker(&mut scope.parameter_bindings, &name, self.parameter);
        marker(&mut scope.list_origin_bindings, &name, self.list_origin);
        marker(
            &mut scope.default_parameter_bindings,
            &name,
            self.default_parameter,
        );
        marker(&mut scope.lexical_functions, &name, self.lexical);
        if let Some(alias) = self.alias {
            scope.function_aliases.insert(name.clone(), alias);
        } else {
            scope.function_aliases.remove(&name);
        }
        if let Some(p) = scope.reference_provenance.as_mut() {
            if let Some(value) = self.provenance {
                p.bindings.insert(name.clone(), value);
            } else {
                p.bindings.remove(&name);
            }
        }
        if let Some(ty) = self.ty {
            scope.bindings.insert(name, ty);
        } else {
            scope.bindings.remove(&name);
        }
    }
}

// An ordinary assignment removes these facts. Keep only the values actually
// removed; after later undo records are replayed, absent facts need no work.
#[derive(Debug)]
pub(crate) struct AssignmentUndo {
    ty: Option<RType>,
    removed_markers: u8,
    alias: Option<String>,
    provenance: Option<BindingProvenance>,
}

impl AssignmentUndo {
    fn restore(self, scope: &mut Scope, name: String) {
        let sets = [
            &mut scope.narrowed_bindings,
            &mut scope.parameter_bindings,
            &mut scope.list_origin_bindings,
            &mut scope.default_parameter_bindings,
            &mut scope.lexical_functions,
        ];
        for (index, set) in sets.into_iter().enumerate() {
            debug_assert!(!set.contains(&name));
            if self.removed_markers & (1 << index) != 0 {
                set.insert(name.clone());
            }
        }
        debug_assert!(!scope.function_aliases.contains_key(&name));
        if let Some(alias) = self.alias {
            scope.function_aliases.insert(name.clone(), alias);
        }
        if let Some(provenance) = self.provenance
            && let Some(table) = scope.reference_provenance.as_mut()
        {
            debug_assert!(!table.bindings.contains_key(&name));
            table.bindings.insert(name.clone(), provenance);
        }
        if let Some(ty) = self.ty {
            scope.bindings.insert(name, ty);
        } else {
            scope.bindings.remove(&name);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MarkerKind {
    ListOrigin,
    Lexical,
    Parameter,
}

#[derive(Debug)]
pub(crate) enum Undo {
    KnownString(String, Option<Arc<str>>),
    KnownStrings(FxMap<String, Arc<str>>),
    Assignment(String, AssignmentUndo),
    Binding(String, BindingState),
    Marker(MarkerKind, String, bool),
    Alias(String, Option<String>),
    Reference(String, Option<BindingProvenance>),
    Provenance(HashMap<String, BindingProvenance>),
    OpsBinding(
        String,
        Option<Arc<infer::ops_chooser::LiteralFunction>>,
        bool,
    ),
    OpsTables(
        FxMap<String, Arc<infer::ops_chooser::LiteralFunction>>,
        FxSet<String>,
    ),
}

pub(crate) struct Mark {
    literal_values_unknown: bool,
    len: usize,
    data_mask_unknown: bool,
    tidy_injection: Option<InjectionMode>,
    search_path_unknown: bool,
    unreachable: bool,
    provenance: Option<(Span, bool, Option<ReferenceBlocker>)>,
    loop_frame: Option<usize>,
    effects_unknown: bool,
    ops_environment_unknown: bool,
    has_escaped_slot_names: bool,
}

pub(crate) type BranchChanges =
    hashbrown::HashMap<String, BindingState, std::collections::hash_map::RandomState>;

pub(crate) struct BranchDelta {
    pub literal_values_unknown: bool,
    pub changed: BranchChanges,
    pub unreachable: bool,
    pub effects_unknown: bool,
    pub ops_environment_unknown: bool,
    pub has_escaped_slot_names: bool,
}

pub(crate) struct BindingView<'a> {
    pub ty: Option<&'a RType>,
    pub narrowed: bool,
    pub list_origin: bool,
    pub default_parameter: bool,
}

impl BranchDelta {
    pub fn binding<'a>(
        &'a self,
        base: &'a Scope,
        name: &str,
        original: Option<&'a RType>,
    ) -> BindingView<'a> {
        if let Some(state) = self.changed.get(name) {
            state.view()
        } else {
            BindingView {
                ty: original,
                narrowed: base.narrowed_bindings.contains(name),
                list_origin: base.has_list_origin(name),
                default_parameter: base.is_default_parameter(name),
            }
        }
    }
}

impl Scope {
    pub(crate) fn clear_ops_binding(&mut self, raw_name: &str) {
        let name = semantic_argument_name(raw_name);
        let function = if self.literal_functions.is_empty() {
            None
        } else {
            self.literal_functions.remove(name)
        };
        let vector = !self.plain_ops_vectors.is_empty() && self.plain_ops_vectors.remove(name);
        if self.snapshot_depth > 0 && (function.is_some() || vector) {
            self.undo
                .push(Undo::OpsBinding(name.to_string(), function, vector));
        }
    }

    pub(crate) fn set_literal_function(
        &mut self,
        name: String,
        function: Arc<infer::ops_chooser::LiteralFunction>,
    ) {
        self.journal_ops_binding(&name);
        self.literal_functions.insert(name, function);
    }

    pub(crate) fn mark_plain_ops_vector(&mut self, name: String) {
        self.journal_ops_binding(&name);
        self.plain_ops_vectors.insert(name);
    }

    fn journal_ops_binding(&mut self, name: &str) {
        if self.snapshot_depth > 0 {
            self.undo.push(Undo::OpsBinding(
                name.to_string(),
                self.literal_functions.get(name).cloned(),
                self.plain_ops_vectors.contains(name),
            ));
        }
    }

    pub(crate) fn clear_ops_facts(&mut self) {
        if self.snapshot_depth > 0
            && (!self.literal_functions.is_empty() || !self.plain_ops_vectors.is_empty())
        {
            self.undo.push(Undo::OpsTables(
                std::mem::take(&mut self.literal_functions),
                std::mem::take(&mut self.plain_ops_vectors),
            ));
        } else {
            self.literal_functions.clear();
            self.plain_ops_vectors.clear();
        }
    }

    pub(crate) fn insert_with_assignment_undo(&mut self, name: String, ty: RType) {
        debug_assert!(self.snapshot_depth > 0);
        self.clear_ops_binding(&name);
        let sets = [
            &mut self.narrowed_bindings,
            &mut self.parameter_bindings,
            &mut self.list_origin_bindings,
            &mut self.default_parameter_bindings,
            &mut self.lexical_functions,
        ];
        let mut removed_markers = 0;
        // Rollback retains empty tables' capacity. Avoid hashing a name for
        // removals that cannot find anything in those retained tables.
        for (index, set) in sets.into_iter().enumerate() {
            if !set.is_empty() && set.remove(&name) {
                removed_markers |= 1 << index;
            }
        }
        let alias = if self.function_aliases.is_empty() {
            None
        } else {
            self.function_aliases.remove(&name)
        };
        let provenance = self.reference_provenance.as_mut().and_then(|table| {
            if table.bindings.is_empty() {
                None
            } else {
                table.bindings.remove(&name)
            }
        });
        let previous = if let Some(current) = self.bindings.get_mut(&name) {
            if current == &ty && removed_markers == 0 && alias.is_none() && provenance.is_none() {
                return;
            }
            Some(std::mem::replace(current, ty))
        } else {
            self.bindings.insert(name.clone(), ty);
            None
        };
        self.undo.push(Undo::Assignment(
            name,
            AssignmentUndo {
                ty: previous,
                removed_markers,
                alias,
                provenance,
            },
        ));
    }

    pub(crate) fn begin_binding_change(&mut self, name: &str) -> Option<usize> {
        if self.snapshot_depth == 0 {
            return None;
        }
        let index = self.undo.len();
        self.undo.push(Undo::Binding(
            name.to_string(),
            BindingState::capture_metadata(self, name),
        ));
        Some(index)
    }

    pub(crate) fn finish_binding_change(&mut self, index: Option<usize>, previous: Option<RType>) {
        if let Some(index) = index {
            if let Undo::Binding(_, state) = &mut self.undo[index] {
                state.ty = previous;
            }
        }
    }

    pub(crate) fn journal_binding(&mut self, name: &str) {
        if self.snapshot_depth > 0 {
            self.undo.push(Undo::Binding(
                name.to_string(),
                BindingState::capture(self, name),
            ));
        }
    }

    pub(crate) fn journal_marker(&mut self, name: &str, kind: MarkerKind) {
        if self.snapshot_depth > 0 {
            let present = match kind {
                MarkerKind::ListOrigin => self.list_origin_bindings.contains(name),
                MarkerKind::Lexical => self.lexical_functions.contains(name),
                MarkerKind::Parameter => self.parameter_bindings.contains(name),
            };
            self.undo
                .push(Undo::Marker(kind, name.to_string(), present));
        }
    }

    pub(crate) fn journal_alias(&mut self, name: &str) {
        if self.snapshot_depth > 0 {
            self.undo.push(Undo::Alias(
                name.to_string(),
                self.function_aliases.get(name).cloned(),
            ));
        }
    }

    pub(crate) fn journal_reference_binding(&mut self, name: &str) {
        if self.snapshot_depth > 0 {
            self.undo.push(Undo::Reference(
                name.to_string(),
                self.reference_provenance
                    .as_ref()
                    .and_then(|p| p.bindings.get(name).cloned()),
            ));
        }
    }

    pub(crate) fn clear_reference_bindings(&mut self) {
        if let Some(provenance) = self.reference_provenance.as_mut() {
            if self.snapshot_depth > 0 {
                self.undo
                    .push(Undo::Provenance(std::mem::take(&mut provenance.bindings)));
            } else {
                provenance.bindings.clear();
            }
        }
    }

    pub(crate) fn begin_snapshot(&mut self) -> Mark {
        self.snapshot_depth += 1;
        Mark {
            literal_values_unknown: self.literal_values_unknown,
            len: self.undo.len(),
            loop_frame: self.loop_frame,
            effects_unknown: self.effects_unknown,
            ops_environment_unknown: self.ops_environment_unknown,
            has_escaped_slot_names: self.has_escaped_slot_names,
            data_mask_unknown: self.data_mask_unknown,
            tidy_injection: self.tidy_injection,
            search_path_unknown: self.search_path_unknown,
            unreachable: self.unreachable,
            provenance: self
                .reference_provenance
                .as_ref()
                .map(|p| (p.owner, p.after_unsafe_read, p.unsafe_read_blocker)),
        }
    }

    pub(crate) fn finish_snapshot(
        &mut self,
        mark: Mark,
        mut changed: BranchChanges,
    ) -> BranchDelta {
        debug_assert!(changed.is_empty());
        let capacity = self.bindings.len().min(self.undo.len() - mark.len);
        // Iterating or clearing a nonempty hash table scans its capacity.
        // A previous dense branch must not make a later sparse delta linear
        // in the old scope size. Dropping this empty cache visits no entries.
        if changed.capacity() > capacity.saturating_mul(4).max(16) {
            changed = BranchChanges::default();
        }
        changed.reserve(capacity);
        // Inspect the latest operation for each name while all final facts
        // are still present. Rollback runs only after capture is complete.
        for undo in self.undo[mark.len..].iter().rev() {
            match undo {
                Undo::Assignment(name, _) => {
                    if let hashbrown::hash_map::EntryRef::Vacant(entry) =
                        changed.entry_ref(name.as_str())
                    {
                        // An ordinary assignment clears all binding metadata.
                        // Later marker/alias/reference changes take the full
                        // capture path because their records are visited first.
                        let state = BindingState {
                            ty: self.bindings.remove(name),
                            ..BindingState::default()
                        };
                        entry.insert(state);
                    }
                }
                Undo::Binding(name, _)
                | Undo::Marker(_, name, _)
                | Undo::Alias(name, _)
                | Undo::Reference(name, _) => {
                    changed
                        .entry_ref(name.as_str())
                        .or_insert_with(|| BindingState::capture(self, name));
                }
                Undo::Provenance(_)
                | Undo::OpsBinding(..)
                | Undo::OpsTables(..)
                | Undo::KnownString(..)
                | Undo::KnownStrings(..) => {}
            }
        }
        let delta = BranchDelta {
            literal_values_unknown: self.literal_values_unknown,
            effects_unknown: self.effects_unknown,
            ops_environment_unknown: self.ops_environment_unknown,
            has_escaped_slot_names: self.has_escaped_slot_names,
            unreachable: self.unreachable,
            changed,
        };
        while self.undo.len() > mark.len {
            match self.undo.pop().unwrap() {
                Undo::KnownString(name, previous) => {
                    if let Some(value) = previous {
                        self.known_strings.insert(name, value);
                    } else {
                        self.known_strings.remove(&name);
                    }
                }
                Undo::KnownStrings(previous) => self.known_strings = previous,
                Undo::Assignment(name, state) => state.restore(self, name),
                Undo::Binding(name, state) => state.restore(self, name),
                Undo::Marker(kind, name, present) => {
                    let set = match kind {
                        MarkerKind::ListOrigin => &mut self.list_origin_bindings,
                        MarkerKind::Lexical => &mut self.lexical_functions,
                        MarkerKind::Parameter => &mut self.parameter_bindings,
                    };
                    if present {
                        set.insert(name);
                    } else {
                        set.remove(&name);
                    }
                }
                Undo::Alias(name, previous) => {
                    if let Some(value) = previous {
                        self.function_aliases.insert(name, value);
                    } else {
                        self.function_aliases.remove(&name);
                    }
                }
                Undo::Reference(name, previous) => {
                    if let Some(p) = self.reference_provenance.as_mut() {
                        if let Some(value) = previous {
                            p.bindings.insert(name, value);
                        } else {
                            p.bindings.remove(&name);
                        }
                    }
                }
                Undo::OpsBinding(name, function, vector) => {
                    self.literal_functions.remove(&name);
                    self.plain_ops_vectors.remove(&name);
                    if let Some(function) = function {
                        self.literal_functions.insert(name.clone(), function);
                    }
                    if vector {
                        self.plain_ops_vectors.insert(name);
                    }
                }
                Undo::OpsTables(functions, vectors) => {
                    self.literal_functions = functions;
                    self.plain_ops_vectors = vectors;
                }
                Undo::Provenance(bindings) => {
                    if let Some(p) = self.reference_provenance.as_mut() {
                        p.bindings = bindings;
                    }
                }
            }
        }
        self.loop_frame = mark.loop_frame;
        self.effects_unknown = mark.effects_unknown;
        self.ops_environment_unknown = mark.ops_environment_unknown;
        self.has_escaped_slot_names = mark.has_escaped_slot_names;
        self.literal_values_unknown = mark.literal_values_unknown;
        self.data_mask_unknown = mark.data_mask_unknown;
        self.tidy_injection = mark.tidy_injection;
        self.search_path_unknown = mark.search_path_unknown;
        self.unreachable = mark.unreachable;
        match mark.provenance {
            None => self.reference_provenance = None,
            Some((owner, after_unsafe_read, unsafe_read_blocker)) => {
                let p = self.reference_provenance.get_or_insert_with(|| {
                    Box::new(ScopeProvenance {
                        owner,
                        after_unsafe_read,
                        unsafe_read_blocker,
                        bindings: HashMap::new(),
                    })
                });
                p.owner = owner;
                p.after_unsafe_read = after_unsafe_read;
                p.unsafe_read_blocker = unsafe_read_blocker;
            }
        }
        self.snapshot_depth -= 1;
        delta
    }

    pub(crate) fn replace_binding_only(&mut self, name: &str, ty: Option<RType>) -> Option<RType> {
        self.journal_binding(name);
        if let Some(ty) = ty {
            self.bindings.insert(name.to_string(), ty)
        } else {
            self.bindings.remove(name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_undo_restores_interleaved_metadata_and_skips_only_full_noops() {
        let mut scope = Scope::default();
        let integer = RType::new(Mode::Integer, Length::One);
        scope.insert("x", integer.clone());
        let initial = format!("{:?}", scope.clone());
        let mark = scope.begin_snapshot();
        scope.insert("x", integer.clone());
        assert!(scope.undo.is_empty());
        scope.mark_list_origin("x");
        let before_assignment = scope.undo.len();
        scope.insert("x", integer);
        assert_eq!(scope.undo.len(), before_assignment + 1);
        scope.set_function_alias("x", "callee".into());
        scope.insert("x", RType::new(Mode::Character, Length::One));
        scope.mark_lexical_function("x");
        let logical = RType::new(Mode::Logical, Length::One);
        scope.insert_narrowed("x", logical.clone());
        let delta = scope.finish_snapshot(mark, BranchChanges::default());
        assert_eq!(delta.changed["x"].ty, Some(logical));
        assert!(delta.changed["x"].narrowed);
        assert_eq!(format!("{:?}", scope), initial);
    }

    #[test]
    fn delta_preserves_removed_values_and_metadata_only_bindings() {
        let mut scope = Scope::default();
        let integer = RType::new(Mode::Integer, Length::One);
        let character = RType::new(Mode::Character, Length::One);
        scope.insert("removed", integer.clone());
        scope.insert("metadata", integer.clone());
        scope.insert("mixed", integer.clone());
        let initial = scope.bindings.clone();
        let mark = scope.begin_snapshot();
        scope.insert("removed", character.clone());
        scope.replace_binding_only("removed", None);
        scope.mark_list_origin("metadata");
        scope.mark_list_origin("mixed");
        scope.insert("mixed", character.clone());
        scope.insert("new", character.clone());
        let delta = scope.finish_snapshot(mark, BranchChanges::default());
        assert_eq!(delta.changed["removed"].ty, None);
        assert_eq!(delta.changed["metadata"].ty, Some(integer));
        assert!(delta.changed["metadata"].list_origin);
        assert_eq!(delta.changed["mixed"].ty, Some(character.clone()));
        assert!(!delta.changed["mixed"].list_origin);
        assert_eq!(delta.changed["new"].ty, Some(character));
        assert_eq!(scope.bindings, initial);
        assert!(scope.list_origin_bindings.is_empty());
        assert!(scope.undo.is_empty());
        assert_eq!(scope.snapshot_depth, 0);
    }

    #[test]
    fn latest_assignment_capture_preserves_later_metadata_changes() {
        let mut scope = Scope::default();
        let integer = RType::new(Mode::Integer, Length::One);
        let character = RType::new(Mode::Character, Length::One);
        scope.insert_parameter_default("cleared", integer);
        scope.mark_list_origin("cleared");
        scope.mark_lexical_function("cleared");
        scope.set_function_alias("cleared", "original".into());
        let initial = BindingState::capture(&scope, "cleared");
        let mark = scope.begin_snapshot();
        scope.insert("cleared", character.clone());
        scope.insert("list", character.clone());
        scope.mark_list_origin("list");
        scope.insert_parameter("parameter", character.clone());
        scope.insert("alias", character);
        scope.set_function_alias("alias", "target".into());
        let expected: Vec<_> = ["cleared", "list", "parameter", "alias"]
            .into_iter()
            .map(|name| (name, BindingState::capture(&scope, name)))
            .collect();
        let delta = scope.finish_snapshot(mark, BranchChanges::default());
        for (name, state) in expected {
            assert_eq!(format!("{:?}", delta.changed[name]), format!("{state:?}"));
        }
        assert_eq!(scope.bindings.len(), 1);
        assert_eq!(
            format!("{:?}", BindingState::capture(&scope, "cleared")),
            format!("{initial:?}")
        );
        assert!(!scope.has_list_origin("list"));
        assert!(!scope.is_parameter("parameter"));
        assert!(scope.function_alias("alias").is_none());
    }

    #[test]
    fn nested_snapshots_restore_markers_and_clones_have_no_history() {
        let mut scope = Scope::default();
        scope.insert_parameter_default("x", RType::new(Mode::Integer, Length::One));
        scope.mark_list_origin("x");
        scope.mark_lexical_function("x");
        scope.set_function_alias("x", "original".into());
        scope.reference_provenance = Some(Box::new(ScopeProvenance {
            owner: Span {
                start: 0,
                end: 1,
                line: 0,
                col: 0,
            },
            after_unsafe_read: false,
            unsafe_read_blocker: None,
            bindings: HashMap::new(),
        }));
        let initial = format!("{:?}", scope.clone());
        let outer = scope.begin_snapshot();
        scope.insert_narrowed("x", RType::new(Mode::Double, Length::One));
        scope.data_mask_unknown = true;
        scope.search_path_unknown = true;
        scope.tidy_injection = Some(InjectionMode::Full);
        scope.unreachable = true;
        scope
            .reference_provenance
            .as_mut()
            .unwrap()
            .after_unsafe_read = true;
        let outer_state = format!("{:?}", scope.clone());
        let inner = scope.begin_snapshot();
        scope.insert("x", RType::new(Mode::Character, Length::One));
        scope.insert("new", RType::unknown());
        scope.replace_binding_only(".", Some(RType::unknown()));
        let clone = scope.clone();
        assert_eq!(clone.snapshot_depth, 0);
        assert!(clone.undo.is_empty());
        let delta = scope.finish_snapshot(inner, BranchChanges::default());
        assert!(delta.changed.contains_key("new"));
        assert_eq!(format!("{:?}", scope.clone()), outer_state);
        scope.finish_snapshot(outer, BranchChanges::default());
        assert_eq!(format!("{:?}", scope), initial);
    }
    fn assert_same_scope(left: &Scope, right: &Scope) {
        assert_eq!(left.bindings, right.bindings);
        assert_eq!(left.narrowed_bindings, right.narrowed_bindings);
        assert_eq!(left.parameter_bindings, right.parameter_bindings);
        assert_eq!(
            left.default_parameter_bindings,
            right.default_parameter_bindings
        );
        assert_eq!(left.list_origin_bindings, right.list_origin_bindings);
        assert_eq!(left.lexical_functions, right.lexical_functions);
        assert_eq!(left.function_aliases, right.function_aliases);
        assert_eq!(left.plain_ops_vectors, right.plain_ops_vectors);
        assert_eq!(left.known_strings, right.known_strings);
        assert_eq!(left.literal_values_unknown, right.literal_values_unknown);
        assert_eq!(left.literal_functions.len(), right.literal_functions.len());
        for (name, function) in &left.literal_functions {
            assert_eq!(
                format!("{function:?}"),
                format!("{:?}", right.literal_functions[name])
            );
        }
        assert_eq!(left.loop_frame, right.loop_frame);
        assert_eq!(left.unreachable, right.unreachable);
        assert_eq!(left.effects_unknown, right.effects_unknown);
        assert_eq!(left.ops_environment_unknown, right.ops_environment_unknown);
        assert_eq!(left.has_escaped_slot_names, right.has_escaped_slot_names);
        assert_eq!(left.search_path_unknown, right.search_path_unknown);
        assert_eq!(left.data_mask_unknown, right.data_mask_unknown);
        assert_eq!(left.tidy_injection, right.tidy_injection);
        match (&left.reference_provenance, &right.reference_provenance) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                assert_eq!(a.owner, b.owner);
                assert_eq!(a.after_unsafe_read, b.after_unsafe_read);
                assert_eq!(a.unsafe_read_blocker, b.unsafe_read_blocker);
                assert_eq!(a.bindings.len(), b.bindings.len());
                for (name, value) in &a.bindings {
                    assert_eq!(format!("{value:?}"), format!("{:?}", b.bindings[name]));
                }
            }
            _ => panic!("reference provenance presence differs"),
        }
    }

    #[test]
    fn canonical_ops_names_and_bulk_effects_restore_across_nested_marks() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser
            .parse(
                "journal.R",
                "`x` <- function(a,b) 1L; y <- structure(1L, class='a')",
            )
            .unwrap();
        let mut checker = Checker::new("journal.R");
        let (_, mut scope) = checker.check_with_scope(&file);
        assert!(scope.literal_functions.contains_key("x"));
        scope.loop_frame = Some(3);
        scope.mark_list_origin("metadata_only");
        let owner = Span {
            start: 2,
            end: 8,
            line: 1,
            col: 2,
        };
        scope.reference_provenance = Some(Box::new(ScopeProvenance {
            owner,
            after_unsafe_read: true,
            unsafe_read_blocker: Some(ReferenceBlocker {
                kind: "prior_read",
                cause: "unsafe_read",
                span: Some(owner),
                scope_span: owner,
            }),
            bindings: HashMap::new(),
        }));
        let initial = scope.clone();
        let outer = scope.begin_snapshot();
        scope.insert("x", RType::scalar(Mode::Character));
        assert!(!scope.literal_functions.contains_key("x"));
        scope.mark_plain_ops_vector("x".into());
        let before_inner = scope.clone();
        let inner = scope.begin_snapshot();
        scope.insert("`x`", RType::scalar(Mode::Logical));
        scope.invalidate_unknown_effects();
        scope.insert("x", RType::scalar(Mode::Integer));
        scope.loop_frame = None;
        scope.unreachable = true;
        let independent = scope.independent_execution_scope();
        assert!(independent.undo.is_empty());
        assert_eq!(independent.snapshot_depth, 0);
        assert_eq!(independent.loop_frame, None);
        assert!(!independent.unreachable);
        let delta = scope.finish_snapshot(inner, BranchChanges::default());
        assert!(delta.effects_unknown && delta.ops_environment_unknown && delta.unreachable);
        assert_same_scope(&scope, &before_inner);
        scope.clear_ops_facts();
        scope.finish_snapshot(outer, BranchChanges::default());
        assert_same_scope(&scope, &initial);
    }

    #[test]
    fn journal_matches_clone_diagnostics_scopes_and_reference_facts() {
        let sources = [
            r"`\x40` <- function(object, name) 1L; if(flag) not_bound@slot else 1L",
            r"x <- list(); if(flag) { `@\x3c-` <- function(object,name,value) 1L; x@slot <- 2L } else x <- list(); x",
            r"while(TRUE) { if(flag) { `@\x3c-` <- function(object,name,value) 1L; break }; break }",
            r"for(i in 1:2) { if(flag) { `@\x3c-` <- function(object,name,value) 1L; next }; x <- 1L }; x",
            "f <- NULL; if (flag) f <- function(x) x else f <- function(x) x+1; f(1)",
            "f <- NULL; if (flag) for(i in integer()) f <- function(x)x else for(i in integer()) f <- function(x)x; f()",
            "f <- NULL; if (is.function(f)) f() else if (flag) f <- function(x)x; f()",
            "x <- 1L; while(TRUE) { if(flag) { x <- list(a=1); break }; x <- list(a=2); break; x <- NULL }; x$a",
            "x <- 1L; while(TRUE) { if(flag) break; unknown(); x <- list(a=2); break }; x$a",
            "for(i in 1:2) { if(flag) next else { x <- 1L; break }; x <- function()NULL }; x$a",
            "while(TRUE) { if(flag) { f <- function() { break }; break }; break }",
            "`x` <- function(a,b) 1L; if(flag) { x <- function(a,b) 2L; if(other) `x` <- function(a,b) 's' }; y <- x(1,2)",
            "x <- 1L; x; if(flag) { x <- 2L; x; unknown(); x <- 3L; x } else { x <- 'a'; x }; x",
            "f <- function(p=1L) { x <- 1L; if(flag) { y <- p; x } else { x <- 2L; x }; x }; f()",
            "x <- list(a=1); if(flag) { x <- x %>% identity(); x$a } else x <- list(a=2); x$a",
        ];
        for source in sources {
            let mut parser = ry_core::RParser::new().unwrap();
            let file = parser.parse("journal.R", source).unwrap();
            let mut clone = Checker::new("journal.R");
            clone.journal_branches = false;
            clone.enable_reference_capture();
            let (expected_diags, expected_scope) = clone.check_with_scope(&file);
            let expected_facts = clone.take_reference_facts();
            let mut journal = Checker::new("journal.R");
            journal.journal_branches = true;
            journal.enable_reference_capture();
            let (actual_diags, actual_scope) = journal.check_with_scope(&file);
            assert_eq!(
                format!("{actual_diags:?}"),
                format!("{expected_diags:?}"),
                "{source}"
            );
            assert_same_scope(&actual_scope, &expected_scope);
            assert_eq!(
                format!("{:?}", journal.take_reference_facts()),
                format!("{expected_facts:?}"),
                "{source}"
            );
        }
    }
    #[test]
    fn fixture_corpus_preserves_full_scope_and_reference_facts() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
        fn collect_fixtures(directory: &std::path::Path, paths: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let kind = entry.file_type().unwrap();
                if kind.is_dir() {
                    collect_fixtures(&path, paths);
                } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "R") {
                    paths.push(path);
                }
            }
        }
        let mut paths = Vec::new();
        collect_fixtures(&directory, &mut paths);
        paths.sort();
        for path in paths {
            let source = std::fs::read_to_string(&path).unwrap();
            let filename = path.to_str().unwrap();
            let mut parser = ry_core::RParser::new().unwrap();
            let file = parser.parse(filename, &source).unwrap();
            let mut clone = Checker::new(filename);
            clone.journal_branches = false;
            clone.enable_reference_capture();
            let (expected_diags, expected_scope) = clone.check_with_scope(&file);
            let expected_facts = clone.take_reference_facts();
            let mut journal = Checker::new(filename);
            journal.journal_branches = true;
            journal.enable_reference_capture();
            let (actual_diags, actual_scope) = journal.check_with_scope(&file);
            assert_eq!(
                format!("{actual_diags:?}"),
                format!("{expected_diags:?}"),
                "{filename}"
            );
            assert_same_scope(&actual_scope, &expected_scope);
            assert_eq!(
                format!("{:?}", journal.take_reference_facts()),
                format!("{expected_facts:?}"),
                "{filename}"
            );
        }
    }
    #[test]
    fn reference_install_and_unsafe_read_order_roll_back_original_blocker() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse("journal.R", "x <- 1L; x").unwrap();
        let Stmt::Assign {
            target: Expr::Ident { span, .. },
            ..
        } = &file.stmts[0]
        else {
            panic!("assignment")
        };
        let mut checker = Checker::new("journal.R");
        checker.enable_reference_capture();
        let (_, mut scope) = checker.check_with_scope(&file);
        let initial = scope.clone();
        let mark = scope.begin_snapshot();
        scope.insert("x", RType::scalar(Mode::Character));
        checker.install_reference_definition(&mut scope, "x", *span, true);
        let before_read = scope.clone();
        let inner = scope.begin_snapshot();
        let read_span = Span {
            start: 19,
            end: 25,
            line: 3,
            col: 2,
        };
        checker.finish_reference_read("untracked", read_span, &mut scope);
        let provenance = scope.reference_provenance.as_ref().unwrap();
        assert_eq!(
            provenance.unsafe_read_blocker.unwrap().span,
            Some(read_span)
        );
        assert!(provenance.after_unsafe_read && provenance.bindings.is_empty());
        scope.insert("x", RType::scalar(Mode::Logical));
        checker.install_reference_definition(&mut scope, "x", *span, true);
        assert!(
            scope
                .reference_provenance
                .as_ref()
                .unwrap()
                .bindings
                .is_empty()
        );
        scope.finish_snapshot(inner, BranchChanges::default());
        assert_same_scope(&scope, &before_read);
        scope.finish_snapshot(mark, BranchChanges::default());
        assert_same_scope(&scope, &initial);
    }
    #[test]
    fn dense_cache_does_not_expand_later_sparse_delta_work() {
        let mut scope = Scope::default();
        for index in 0..4096 {
            scope.insert(format!("x{index}"), RType::scalar(Mode::Integer));
        }
        let dense = scope.begin_snapshot();
        for index in 0..4096 {
            scope.insert(format!("x{index}"), RType::scalar(Mode::Character));
        }
        let mut cache = scope
            .finish_snapshot(dense, BranchChanges::default())
            .changed;
        assert!(cache.capacity() >= 4096);
        cache.clear();
        let sparse = scope.begin_snapshot();
        scope.insert("x0", RType::scalar(Mode::Logical));
        let sparse = scope.finish_snapshot(sparse, cache);
        assert_eq!(sparse.changed.len(), 1);
        assert!(sparse.changed.capacity() <= 16);
        assert_eq!(
            sparse.changed["x0"].ty.as_ref().unwrap().mode,
            Mode::Logical
        );
        assert_eq!(scope.get("x0").unwrap().mode, Mode::Integer);
    }

    #[test]
    fn escaped_slot_flag_nested_snapshots_restore_exact_scope_state() {
        let name = r"`@\x3c-`";
        let mut scope = Scope::default();
        scope.insert("ordinary", RType::scalar(Mode::Integer));
        let initial = scope.clone();
        let outer = scope.begin_snapshot();
        scope.insert("ordinary", RType::scalar(Mode::Double));
        let before_inner = scope.clone();
        let inner = scope.begin_snapshot();
        scope.insert(name, RType::scalar(Mode::Integer));
        scope.insert(name, RType::scalar(Mode::Logical));
        scope.replace_binding_only(name, None);
        let delta = scope.finish_snapshot(inner, BranchChanges::default());
        assert!(delta.has_escaped_slot_names);
        assert!(delta.changed[name].ty.is_none());
        assert_same_scope(&scope, &before_inner);
        assert!(!scope.has_escaped_slot_names);
        let delta = scope.finish_snapshot(outer, BranchChanges::default());
        assert!(!delta.has_escaped_slot_names);
        assert_same_scope(&scope, &initial);

        let outer = scope.begin_snapshot();
        scope.insert(name, RType::unknown());
        let before_inner = scope.clone();
        assert!(before_inner.has_escaped_slot_names);
        assert!(scope.independent_execution_scope().has_escaped_slot_names);
        let inner = scope.begin_snapshot();
        scope.invalidate_unknown_effects();
        scope.insert_parameter_default(name, RType::scalar(Mode::Integer));
        scope.insert_narrowed(name, RType::scalar(Mode::Logical));
        scope.replace_binding_only(name, None);
        assert!(
            scope
                .finish_snapshot(inner, BranchChanges::default())
                .has_escaped_slot_names
        );
        assert_same_scope(&scope, &before_inner);
        assert!(
            scope
                .finish_snapshot(outer, BranchChanges::default())
                .has_escaped_slot_names
        );
        assert_same_scope(&scope, &initial);
    }
}
