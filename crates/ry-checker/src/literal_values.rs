//! Character values known within the current execution path.
use super::*;
use crate::scope_journal::Undo;

impl Scope {
    pub(crate) fn invalidate_literal_values(&mut self) {
        self.clear_known_strings();
        self.literal_values_unknown = true;
    }

    pub(crate) fn invalidate_literal_values_for_dispatch(&mut self, value: &RType) {
        if value.class.is_unknown()
            || value.class.has_known_class()
            || matches!(value.mode, Mode::Opaque | Mode::Union)
        {
            self.invalidate_literal_values();
        }
    }

    pub(crate) fn known_string(&self, name: &str) -> Option<&str> {
        self.known_strings
            .get(semantic_argument_name(name))
            .map(AsRef::as_ref)
    }

    pub(crate) fn set_known_string(&mut self, name: &str, value: Arc<str>) {
        if self.effects_unknown || self.literal_values_unknown {
            return;
        }
        let name = semantic_argument_name(name).to_string();
        let previous = self.known_strings.insert(name.clone(), value);
        if self.snapshot_depth > 0 {
            self.undo.push(Undo::KnownString(name, previous));
        }
    }

    pub(crate) fn clear_known_string(&mut self, name: &str) {
        if self.known_strings.is_empty() {
            return;
        }
        let name = semantic_argument_name(name);
        if let Some(value) = self.known_strings.remove(name)
            && self.snapshot_depth > 0
        {
            self.undo
                .push(Undo::KnownString(name.to_string(), Some(value)));
        }
    }

    pub(crate) fn clear_known_strings(&mut self) {
        if self.snapshot_depth > 0 && !self.known_strings.is_empty() {
            self.undo
                .push(Undo::KnownStrings(std::mem::take(&mut self.known_strings)));
        } else {
            self.known_strings.clear();
        }
    }
}
