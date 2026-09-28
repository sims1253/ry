//! Bounded presentation of inferred facts. These strings are not declarations.

use std::fmt::{self, Write};

use crate::types::{Length, Mode, RType};

const HINT_BYTES: usize = 160;
const DETAIL_BYTES: usize = 8192;
const MAX_DEPTH: usize = 16;

/// A rendered view of an inferred type. `truncated` covers both output and
/// nesting limits, so consumers never present incomplete text as exhaustive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeView {
    pub text: String,
    pub truncated: bool,
}

/// Short editor label. Omitted schema entries are counted as *known* fields;
/// openness remains a separate property.
pub fn compact_hint(ty: &RType) -> TypeView {
    let mut renderer = Renderer::new(HINT_BYTES, true);
    renderer.ty(ty, 0);
    renderer.finish()
}

/// All represented facts, subject to an explicit output budget. A truncated
/// result points consumers to `dump-facts` for structured data.
pub fn expanded_type(ty: &RType) -> TypeView {
    let mut renderer = Renderer::new(DETAIL_BYTES, false);
    renderer.ty(ty, 0);
    renderer.finish()
}

struct Renderer {
    text: String,
    max_bytes: usize,
    compact: bool,
    truncated: bool,
}

impl Renderer {
    fn new(max_bytes: usize, compact: bool) -> Self {
        Self {
            text: String::new(),
            max_bytes,
            compact,
            truncated: false,
        }
    }

    fn push(&mut self, value: &str) {
        if self.truncated {
            return;
        }
        if value.len() > self.max_bytes.saturating_sub(self.text.len()) {
            self.truncated = true;
            return;
        }
        self.text.push_str(value);
    }

    fn escaped_name(&mut self, name: &str) {
        // Debug escaping writes through the bounded formatter. Formatting a
        // full name first would allocate and scan source-sized input even
        // after the display budget had been exhausted.
        let _ = write!(self, "{name:?}");
    }

    fn finish(mut self) -> TypeView {
        if self.truncated {
            // Reserve the visible marker even when the buffer is full.
            while self.text.len() + "…".len() > self.max_bytes {
                self.text.pop();
            }
            self.text.push('…');
        }
        TypeView {
            text: self.text,
            truncated: self.truncated,
        }
    }

    fn ty(&mut self, ty: &RType, depth: usize) {
        if self.truncated {
            return;
        }
        if depth >= MAX_DEPTH {
            self.push("[nested facts omitted]");
            self.truncated = true;
            return;
        }
        self.push(&format!("{}<len={}>", ty.mode, length(ty.length)));
        if ty.mode == Mode::Union {
            self.push("[");
            if let Some(members) = &ty.members {
                let shown = if self.compact {
                    members.len().min(3)
                } else {
                    members.len()
                };
                for (index, member) in members.iter().take(shown).enumerate() {
                    if self.truncated {
                        break;
                    }
                    if index != 0 {
                        self.push(", ");
                    }
                    self.ty(member, depth + 1);
                }
                if shown < members.len() {
                    self.push(&format!(", +{} known alternatives", members.len() - shown));
                }
            } else {
                self.push("?");
            }
            self.push("]");
        }
        if ty.class.known {
            if ty.class.len > 0 || !self.compact {
                self.push(if self.compact { ":" } else { " class=[" });
                for (index, name) in ty
                    .class
                    .names
                    .iter()
                    .take(usize::from(ty.class.len))
                    .enumerate()
                {
                    if self.truncated {
                        break;
                    }
                    if index != 0 {
                        self.push(",");
                    }
                    match name {
                        Some(name) => self.escaped_name(name),
                        None => self.push("?"),
                    }
                }
                if !self.compact {
                    self.push("]");
                    if ty.class.len >= 4 {
                        self.push(" (may be truncated at 4 classes)");
                    }
                } else if ty.class.len >= 4 {
                    self.push("+");
                }
            }
        } else {
            self.push(if self.compact { ":?" } else { " class=?" });
        }
        match &ty.columns {
            Some(schema) => {
                self.push("{");
                let shown = if self.compact {
                    schema.columns.len().min(3)
                } else {
                    schema.columns.len()
                };
                for (index, (name, field_ty)) in schema.columns.iter().take(shown).enumerate() {
                    if self.truncated {
                        break;
                    }
                    if index != 0 {
                        self.push(", ");
                    }
                    self.escaped_name(name);
                    self.push(": ");
                    self.ty(field_ty, depth + 1);
                }
                if shown < schema.columns.len() {
                    if shown != 0 {
                        self.push(", ");
                    }
                    self.push(&format!("+{} known fields", schema.columns.len() - shown));
                }
                self.push(if schema.complete {
                    "; complete}"
                } else {
                    "; open}"
                });
                if !self.compact {
                    self.push(if schema.locally_constructed {
                        " (locally constructed: true)"
                    } else {
                        " (locally constructed: false)"
                    });
                }
            }
            None if !self.compact => self.push(" columns=?"),
            None => {}
        }
        match &ty.fn_sig {
            Some(signature) => {
                if self.compact {
                    self.push(" (params partial) -> ");
                    self.ty(&signature.return_type, depth + 1);
                } else {
                    self.push(" function=partial(params=[");
                    for (index, param) in signature.params.iter().enumerate() {
                        if self.truncated {
                            break;
                        }
                        if index != 0 {
                            self.push(", ");
                        }
                        self.ty(param, depth + 1);
                    }
                    self.push("], return=");
                    self.ty(&signature.return_type, depth + 1);
                    self.push(")");
                }
            }
            None if matches!(ty.mode, Mode::Function | Mode::Union) => {
                self.push(if self.compact {
                    " (params?, return?)"
                } else {
                    " function=?"
                });
            }
            None => {}
        }
    }
}

impl Write for Renderer {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.truncated {
            return Err(fmt::Error);
        }
        self.push(value);
        if self.truncated {
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

fn length(length: Length) -> String {
    match length {
        Length::Zero => "0".into(),
        Length::One => "1".into(),
        Length::Known(value) => value.to_string(),
        Length::Unknown => "?".into(),
        Length::Nonempty => "1+".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::types::{ClassVector, ColumnSchema, FunctionSignature};

    #[test]
    fn wide_schemas_show_hidden_known_fields_separately_from_openness() {
        let columns = (0..5)
            .map(|n| (format!("field{n}"), RType::scalar(Mode::Integer)))
            .collect();
        let closed =
            RType::new(Mode::List, Length::Known(5)).with_columns(Arc::new(ColumnSchema {
                columns,
                complete: true,
                locally_constructed: false,
            }));
        let mut open = closed.clone();
        Arc::make_mut(open.columns.as_mut().unwrap()).complete = false;
        let mut different_fourth = closed.clone();
        Arc::make_mut(different_fourth.columns.as_mut().unwrap()).columns[3].1 =
            RType::scalar(Mode::Character);
        assert!(
            compact_hint(&closed)
                .text
                .contains("+2 known fields; complete")
        );
        assert!(compact_hint(&open).text.contains("+2 known fields; open"));
        assert!(expanded_type(&closed).text.contains("\"field4\""));
        assert!(!expanded_type(&closed).truncated);
        assert_eq!(compact_hint(&closed), compact_hint(&different_fourth));
        assert_ne!(expanded_type(&closed), expanded_type(&different_fourth));
    }

    #[test]
    fn expanded_view_distinguishes_absent_and_empty_metadata() {
        let unknown = expanded_type(&RType::unknown()).text;
        assert!(unknown.contains("class=? columns=?"));
        let empty = RType::new(Mode::List, Length::Zero).with_columns(Arc::new(ColumnSchema {
            columns: vec![],
            complete: true,
            locally_constructed: false,
        }));
        assert!(expanded_type(&empty).text.contains("class=[]{; complete}"));
        let function = RType::scalar(Mode::Function).with_fn_sig(Arc::new(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Integer)),
        }));
        assert!(
            expanded_type(&function)
                .text
                .contains("function=partial(params=[], return=")
        );
        assert!(
            compact_hint(&RType::scalar(Mode::Function))
                .text
                .contains("params?")
        );
    }

    #[test]
    fn union_view_keeps_its_common_metadata() {
        let base = RType::union(Arc::from([
            RType::scalar(Mode::Integer),
            RType::scalar(Mode::Character),
        ]));
        let tagged = base.clone().with_class(ClassVector::single("tagged"));
        let tagged = tagged.with_columns(Arc::new(ColumnSchema {
            columns: vec![("value".into(), RType::scalar(Mode::Integer))],
            complete: false,
            locally_constructed: false,
        }));
        let plain = expanded_type(&base);
        let decorated = expanded_type(&tagged);
        assert!(!decorated.truncated);
        assert_ne!(plain, decorated);
        assert!(decorated.text.contains("union<len=1>["));
        assert!(decorated.text.contains("class=[\"tagged\"]"));
        assert!(decorated.text.contains("\"value\": integer"));
        assert!(decorated.text.contains("; open}"));
    }

    #[test]
    fn nesting_and_long_names_mark_incomplete_views() {
        let mut ty = RType::scalar(Mode::Integer);
        for _ in 0..20 {
            ty = RType::new(Mode::List, Length::One).with_columns(Arc::new(ColumnSchema {
                columns: vec![("x".into(), ty)],
                complete: true,
                locally_constructed: false,
            }));
        }
        assert!(expanded_type(&ty).truncated);
        let long = RType::scalar(Mode::Integer).with_class(ClassVector::single(&"x".repeat(10000)));
        let view = expanded_type(&long);
        assert!(view.truncated);
        assert!(view.text.ends_with('…'));
        assert!(view.text.len() <= DETAIL_BYTES);

        let escaped = RType::new(Mode::List, Length::One).with_columns(Arc::new(ColumnSchema {
            columns: vec![("a\"b\n".into(), RType::scalar(Mode::Integer))],
            complete: true,
            locally_constructed: true,
        }));
        assert!(expanded_type(&escaped).text.contains("\"a\\\"b\\n\""));
    }

    #[test]
    fn wide_schema_and_source_sized_name_obey_output_budget() {
        let huge_name = "\\\"".repeat(200_000);
        let mut columns = vec![(huge_name, RType::scalar(Mode::Integer))];
        columns.extend(
            (0..100_000).map(|index| (format!("late{index}"), RType::scalar(Mode::Integer))),
        );
        let ty = RType::new(Mode::List, Length::Unknown).with_columns(Arc::new(ColumnSchema {
            columns,
            complete: true,
            locally_constructed: true,
        }));
        let started = std::time::Instant::now();
        let view = expanded_type(&ty);
        assert!(view.truncated);
        assert!(view.text.len() <= DETAIL_BYTES);
        assert!(view.text.ends_with('…'));
        assert!(started.elapsed().as_secs() < 2);
    }
}
