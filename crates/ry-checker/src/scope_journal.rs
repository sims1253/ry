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
        Self {
            ty: scope.get(name).cloned(),
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

#[derive(Debug)]
pub(crate) enum Undo {
    Binding(String, BindingState),
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

impl BranchDelta {
    pub fn binding(&self, base: &Scope, name: &str) -> BindingState {
        self.changed
            .get(name)
            .cloned()
            .unwrap_or_else(|| BindingState::capture(base, name))
    }
}

impl Scope {
    pub(crate) fn journal_binding(&mut self, name: &str) {
        if self.snapshot_depth > 0 {
            self.undo.push(Undo::Binding(
                name.to_string(),
                BindingState::capture(self, name),
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
            if let Undo::Binding(name, _) = undo {
                names.insert(name.clone());
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
                Undo::Binding(name, state) => state.restore(self, name),
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
