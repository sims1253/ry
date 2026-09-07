//! Isolated snapshot experiment; enabled only by RY_SCOPE_JOURNAL=1.
use super::*;
use crate::reference_facts::{BindingProvenance, ScopeProvenance};

pub(crate) fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("RY_SCOPE_JOURNAL").as_deref() == Ok("1"))
}

#[derive(Debug, Clone)]
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
        fn marker(set: &mut HashSet<String>, name: &str, present: bool) {
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
    Assignment(String, AssignmentUndo),
    Binding(String, BindingState),
    Marker(MarkerKind, String, bool),
    Alias(String, Option<String>),
    Reference(String, Option<BindingProvenance>),
    Provenance(HashMap<String, BindingProvenance>),
}

pub(crate) struct Mark {
    len: usize,
    data_mask_unknown: bool,
    tidy_injection: Option<InjectionMode>,
    search_path_unknown: bool,
    unreachable: bool,
    provenance: Option<(Span, bool)>,
}

pub(crate) struct BranchDelta {
    pub changed: HashMap<String, BindingState>,
    pub unreachable: bool,
}

pub(crate) struct BindingView<'a> {
    pub ty: Option<&'a RType>,
    pub narrowed: bool,
    pub list_origin: bool,
    pub default_parameter: bool,
}

impl BranchDelta {
    pub fn binding<'a>(&'a self, base: &'a Scope, name: &str) -> BindingView<'a> {
        if let Some(state) = self.changed.get(name) {
            BindingView {
                ty: state.ty.as_ref(),
                narrowed: state.narrowed,
                list_origin: state.list_origin,
                default_parameter: state.default_parameter,
            }
        } else {
            BindingView {
                ty: base.get(name),
                narrowed: base.narrowed_bindings.contains(name),
                list_origin: base.has_list_origin(name),
                default_parameter: base.is_default_parameter(name),
            }
        }
    }
}

impl Scope {
    pub(crate) fn insert_with_assignment_undo(&mut self, name: String, ty: RType) {
        debug_assert!(self.snapshot_depth > 0);
        let same_type = self.get(&name) == Some(&ty);
        let sets = [
            &mut self.narrowed_bindings,
            &mut self.parameter_bindings,
            &mut self.list_origin_bindings,
            &mut self.default_parameter_bindings,
            &mut self.lexical_functions,
        ];
        let mut removed_markers = 0;
        for (index, set) in sets.into_iter().enumerate() {
            if set.remove(&name) {
                removed_markers |= 1 << index;
            }
        }
        let alias = self.function_aliases.remove(&name);
        let provenance = self
            .reference_provenance
            .as_mut()
            .and_then(|table| table.bindings.remove(&name));
        if same_type && removed_markers == 0 && alias.is_none() && provenance.is_none() {
            self.bindings.insert(name, ty);
            return;
        }
        let previous = self.bindings.insert(name.clone(), ty);
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
            len: self.undo.len(),
            data_mask_unknown: self.data_mask_unknown,
            tidy_injection: self.tidy_injection,
            search_path_unknown: self.search_path_unknown,
            unreachable: self.unreachable,
            provenance: self
                .reference_provenance
                .as_ref()
                .map(|p| (p.owner, p.after_unsafe_read)),
        }
    }

    pub(crate) fn finish_snapshot(&mut self, mark: Mark) -> BranchDelta {
        let mut names = HashSet::new();
        for undo in &self.undo[mark.len..] {
            match undo {
                Undo::Assignment(name, _)
                | Undo::Binding(name, _)
                | Undo::Marker(_, name, _)
                | Undo::Alias(name, _)
                | Undo::Reference(name, _) => {
                    names.insert(name.clone());
                }
                Undo::Provenance(_) => {}
            }
        }
        let delta = BranchDelta {
            unreachable: self.unreachable,
            changed: names
                .into_iter()
                .map(|name| {
                    let state = BindingState::capture(self, &name);
                    (name, state)
                })
                .collect(),
        };
        while self.undo.len() > mark.len {
            match self.undo.pop().unwrap() {
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
                Undo::Provenance(bindings) => {
                    if let Some(p) = self.reference_provenance.as_mut() {
                        p.bindings = bindings;
                    }
                }
            }
        }
        self.data_mask_unknown = mark.data_mask_unknown;
        self.tidy_injection = mark.tidy_injection;
        self.search_path_unknown = mark.search_path_unknown;
        self.unreachable = mark.unreachable;
        match mark.provenance {
            None => self.reference_provenance = None,
            Some((owner, after_unsafe_read)) => {
                let p = self.reference_provenance.get_or_insert_with(|| {
                    Box::new(ScopeProvenance {
                        owner,
                        after_unsafe_read,
                        bindings: HashMap::new(),
                    })
                });
                p.owner = owner;
                p.after_unsafe_read = after_unsafe_read;
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
        let delta = scope.finish_snapshot(mark);
        assert_eq!(delta.changed["x"].ty, Some(logical));
        assert!(delta.changed["x"].narrowed);
        assert_eq!(format!("{:?}", scope), initial);
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
        let delta = scope.finish_snapshot(inner);
        assert!(delta.changed.contains_key("new"));
        assert_eq!(format!("{:?}", scope.clone()), outer_state);
        scope.finish_snapshot(outer);
        assert_eq!(format!("{:?}", scope), initial);
    }
}
