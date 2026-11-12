//! Lattice laws of `RType::join`.
//!
//! `join` merges the types of control-flow branches. The fixpoint loop in
//! `ry-checker` compares joined types with `==` to detect convergence, so
//! the result must not depend on how the branches are grouped or ordered.

use std::sync::Arc;

use proptest::prelude::*;
use ry_core::types::{ClassVector, ColumnSchema, FunctionSignature, ValueFacts};
use ry_core::{Length, Mode, RType};

fn mode() -> impl Strategy<Value = Mode> {
    prop_oneof![
        Just(Mode::Logical),
        Just(Mode::Integer),
        Just(Mode::Double),
        Just(Mode::Complex),
        Just(Mode::Character),
        Just(Mode::Raw),
        Just(Mode::List),
        Just(Mode::Null),
        Just(Mode::Function),
        Just(Mode::Environment),
        Just(Mode::Language),
    ]
}

fn length() -> impl Strategy<Value = Length> {
    prop_oneof![
        Just(Length::Zero),
        Just(Length::One),
        (1usize..4).prop_map(Length::Known),
        Just(Length::Unknown),
        Just(Length::Nonempty),
        (2usize..4).prop_map(Length::AtLeast),
    ]
}

fn class() -> impl Strategy<Value = ClassVector> {
    prop_oneof![
        4 => Just(ClassVector::empty()),
        1 => Just(ClassVector::unknown()),
        1 => Just(ClassVector::single("foo")),
        1 => Just(ClassVector::single("bar")),
        1 => Just(ClassVector::guard("foo")),
        1 => Just(ClassVector::from_slice(&["foo", "bar"])),
        1 => Just(ClassVector::storage_proxy()),
    ]
}

fn value_facts() -> impl Strategy<Value = ValueFacts> {
    prop_oneof![
        3 => Just(ValueFacts::default()),
        1 => (-2i32..3).prop_map(|n| ValueFacts::exact_number(f64::from(n))),
        1 => Just(ValueFacts::exact_number(f64::NAN)),
    ]
}

fn leaf() -> BoxedStrategy<RType> {
    prop_oneof![
        8 => (mode(), length(), class(), value_facts())
            .prop_map(|(m, l, c, f)| RType::new(m, l).with_class(c).with_value_facts(f)),
        1 => Just(RType::unknown()),
    ]
    .boxed()
}

/// Leaves, optionally carrying a column schema or a function signature
/// built from leaves.
fn decorated() -> BoxedStrategy<RType> {
    prop_oneof![
        6 => leaf(),
        1 => (prop::collection::vec(leaf(), 1..3), any::<bool>()).prop_map(|(cols, complete)| {
            let schema = ColumnSchema {
                columns: cols
                    .into_iter()
                    .enumerate()
                    .map(|(i, t)| (format!("c{}", i % 2), t))
                    .collect(),
                complete,
                locally_constructed: false,
            };
            RType::new(Mode::List, Length::Known(2)).with_columns(Arc::new(schema))
        }),
        1 => (leaf(), prop::option::of(leaf())).prop_map(|(ret, param)| {
            let sig = FunctionSignature {
                params: param.into_iter().collect(),
                return_type: Box::new(ret),
            };
            RType::scalar(Mode::Function).with_fn_sig(Arc::new(sig))
        }),
    ]
    .boxed()
}

/// Any type `join` can produce, including unions built by joining leaves.
fn rtype() -> BoxedStrategy<RType> {
    prop_oneof![
        3 => decorated(),
        1 => prop::collection::vec(decorated(), 2..5)
            .prop_map(|ts| ts.into_iter().reduce(RType::join).unwrap()),
    ]
    .boxed()
}

fn join(a: &RType, b: &RType) -> RType {
    a.clone().join(b.clone())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn join_is_idempotent(a in rtype()) {
        prop_assert_eq!(join(&a, &a), a);
    }

    #[test]
    fn join_is_commutative(a in rtype(), b in rtype()) {
        let (l, r) = (join(&a, &b), join(&b, &a));
        prop_assert_eq!(&l, &r, "{} v {} = {} but reversed = {}", a, b, l, r);
    }

    #[test]
    fn join_is_associative(a in rtype(), b in rtype(), c in rtype()) {
        let (l, r) = (join(&join(&a, &b), &c), join(&a, &join(&b, &c)));
        prop_assert_eq!(&l, &r, "({} v {}) v {} = {} but {} v ({} v {}) = {}", a, b, c, l, a, b, c, r);
    }

    #[test]
    fn unknown_absorbs(a in rtype()) {
        let joined = join(&a, &RType::unknown());
        prop_assert!(joined.mode.is_unknown(), "{} v unknown = {}", a, joined);
        prop_assert_eq!(&joined, &join(&RType::unknown(), &a));
    }
}
