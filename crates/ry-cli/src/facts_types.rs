//! Structured views of checker types. This module does not infer new facts.

use ry_core::RType;
use ry_core::types::{Length, Mode};
use serde_json::{Value, json};

pub(crate) fn export_type(ty: &RType) -> Value {
    let mode = match ty.mode {
        Mode::Logical => "logical",
        Mode::Integer => "integer",
        Mode::Double => "double",
        Mode::Complex => "complex",
        Mode::Character => "character",
        Mode::Raw => "raw",
        Mode::List => "list",
        Mode::Null => "null",
        Mode::Function => "function",
        Mode::Opaque => "opaque",
        Mode::Union => "union",
    };
    let length = match ty.length {
        Length::Zero => json!({"kind": "known", "value": 0}),
        Length::One => json!({"kind": "known", "value": 1}),
        Length::Known(value) => json!({"kind": "known", "value": value}),
        Length::Unknown => json!({"kind": "unknown"}),
    };
    let class_names: Vec<_> = ty
        .class
        .names
        .iter()
        .take(usize::from(ty.class.len))
        .map(|name| name.as_deref())
        .collect();
    let class = json!({
        "kind": if ty.class.known { "known" } else { "unknown" },
        "names": class_names,
        // The checker stores only the first four classes. At capacity it
        // cannot tell us whether the original class vector was longer.
        "capacity": 4,
        "may_be_truncated": ty.class.len >= 4,
    });
    let columns = match &ty.columns {
        None => json!({"kind": "unknown"}),
        Some(schema) => json!({
            "kind": if schema.complete { "complete" } else { "partial" },
            "entries": schema.columns.iter().map(|(name, ty)| {
                json!({"key": name, "type": export_type(ty)})
            }).collect::<Vec<_>>(),
            "locally_constructed": schema.locally_constructed,
        }),
    };
    let members = match &ty.members {
        Some(members) => {
            let mut types: Vec<_> = members.iter().map(export_type).collect();
            // Union order has no meaning. Sort the entire structured value,
            // including nested members, instead of its abbreviated Display.
            types.sort_by_cached_key(Value::to_string);
            json!({"kind": "known", "types": types})
        }
        None if ty.mode == Mode::Union => json!({"kind": "unknown"}),
        None => json!({"kind": "not_applicable"}),
    };
    let function = match &ty.fn_sig {
        Some(signature) => json!({
            "kind": "partial",
            "params": signature.params.iter().map(export_type).collect::<Vec<_>>(),
            "parameters_complete": false,
            "return_type": export_type(&signature.return_type),
        }),
        None if matches!(ty.mode, Mode::Function | Mode::Opaque | Mode::Union) => {
            json!({"kind": "unknown"})
        }
        None => json!({"kind": "not_applicable"}),
    };
    json!({
        "kind": "r_type",
        "mode": mode,
        "length": length,
        "class": class,
        "columns": columns,
        "members": members,
        "function": function,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_core::types::{ClassVector, ColumnSchema, FunctionSignature};
    use std::sync::Arc;

    #[test]
    fn unknown_empty_and_partial_columns_are_distinct() {
        let unknown = export_type(&RType::unknown());
        let empty = RType::new(Mode::List, Length::Zero).with_columns(Arc::new(ColumnSchema {
            columns: vec![],
            complete: true,
            locally_constructed: true,
        }));
        let partial =
            RType::new(Mode::List, Length::Unknown).with_columns(Arc::new(ColumnSchema {
                columns: vec![],
                complete: false,
                locally_constructed: false,
            }));
        assert_eq!(unknown["columns"], json!({"kind": "unknown"}));
        assert_eq!(export_type(&empty)["columns"]["kind"], "complete");
        assert_eq!(export_type(&empty)["columns"]["entries"], json!([]));
        assert_eq!(export_type(&partial)["columns"]["kind"], "partial");
        assert_eq!(export_type(&partial)["columns"]["entries"], json!([]));
        assert_eq!(unknown["function"]["kind"], "unknown");
    }

    #[test]
    fn classes_preserve_order_unknownness_and_capacity_limit() {
        assert_eq!(export_type(&RType::unknown())["class"]["kind"], "unknown");
        let plain = RType::scalar(Mode::Integer);
        assert_eq!(export_type(&plain)["class"]["kind"], "known");
        assert_eq!(export_type(&plain)["class"]["names"], json!([]));
        let classed = plain.with_class(ClassVector::from_slice(&["z", "a", "b", "c", "d"]));
        let class = &export_type(&classed)["class"];
        assert_eq!(class["names"], json!(["z", "a", "b", "c"]));
        assert_eq!(class["may_be_truncated"], true);
    }

    #[test]
    fn nested_columns_preserve_order_and_duplicate_names() {
        let leaf = RType::scalar(Mode::Integer);
        let record =
            RType::new(Mode::List, Length::Known(3)).with_columns(Arc::new(ColumnSchema {
                columns: vec![
                    ("z".into(), leaf.clone()),
                    ("a".into(), RType::unknown()),
                    ("z".into(), leaf),
                ],
                complete: false,
                locally_constructed: true,
            }));
        let value = export_type(&record);
        let entries = value["columns"]["entries"].as_array().unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry["key"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["z", "a", "z"]
        );
        assert_eq!(entries[1]["type"]["mode"], "opaque");
        assert_eq!(value["length"], json!({"kind": "known", "value": 3}));
    }

    #[test]
    fn column_keys_do_not_claim_to_be_actual_field_names() {
        // The checker uses this key for an unnamed slot but can also receive
        // the same spelling as an explicit name. RType cannot distinguish them.
        let ty = RType::new(Mode::List, Length::One).with_columns(Arc::new(ColumnSchema {
            columns: vec![("[[1]]".into(), RType::scalar(Mode::Integer))],
            complete: true,
            locally_constructed: true,
        }));
        let value = export_type(&ty);
        let entry = &value["columns"]["entries"][0];
        assert_eq!(entry["key"], "[[1]]");
        assert!(entry.get("name").is_none());
    }

    #[test]
    fn union_order_is_canonical_and_missing_members_are_unknown() {
        let a = RType::scalar(Mode::Integer);
        let b = RType::scalar(Mode::Character);
        let forward = RType::union(Arc::from([a.clone(), b.clone()]));
        let reverse = RType::union(Arc::from([b, a]));
        assert_eq!(export_type(&forward), export_type(&reverse));
        let malformed = RType {
            members: None,
            ..forward
        };
        assert_eq!(export_type(&malformed)["members"]["kind"], "unknown");
    }

    #[test]
    fn function_parameters_are_partial_even_when_empty() {
        let opaque = RType::scalar(Mode::Function);
        assert_eq!(export_type(&opaque)["function"]["kind"], "unknown");
        let closure = opaque.with_fn_sig(Arc::new(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Integer)),
        }));
        let value = export_type(&closure);
        assert_eq!(value["function"]["kind"], "partial");
        assert_eq!(value["function"]["parameters_complete"], false);
        assert_eq!(value["function"]["params"], json!([]));
        assert_eq!(value["function"]["return_type"]["mode"], "integer");
    }

    #[test]
    fn every_mode_and_length_has_a_stable_tag() {
        for (mode, tag) in [
            (Mode::Logical, "logical"),
            (Mode::Integer, "integer"),
            (Mode::Double, "double"),
            (Mode::Complex, "complex"),
            (Mode::Character, "character"),
            (Mode::Raw, "raw"),
            (Mode::List, "list"),
            (Mode::Null, "null"),
            (Mode::Function, "function"),
            (Mode::Opaque, "opaque"),
            (Mode::Union, "union"),
        ] {
            assert_eq!(export_type(&RType::new(mode, Length::Unknown))["mode"], tag);
        }
        for (length, value) in [(Length::Zero, 0), (Length::One, 1), (Length::Known(99), 99)] {
            assert_eq!(
                export_type(&RType::new(Mode::Integer, length))["length"],
                json!({"kind": "known", "value": value})
            );
        }
        assert_eq!(
            export_type(&RType::unknown())["length"],
            json!({"kind": "unknown"})
        );
    }
}
