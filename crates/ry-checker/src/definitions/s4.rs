//! S4 method selection: which project method a generic call provably reaches.
//!
//! A method is selected only under one of two proofs. Every other call
//! that project methods may answer is [`S4Selection::Uncertain`].
//!
//! - exact: every dispatch position has an exactly known class (or is
//!   missing) and one method's signature names exactly those classes. R's
//!   total distance is then zero, and an equal competitor would need the
//!   same signature, which `setMethod` replaces.
//! - receiver-only: every method has `ANY` after the first position, and
//!   one method names the receiver's exact class.
//!
//! An exactly known class comes only from the argument expression itself:
//! a `new("A")` call proven to reach `methods::new`, or a plain atomic
//! literal. No class inheritance is modeled, so an absent relation never
//! excludes a method.

use super::{Callee, Definitions};
use crate::infer::match_arguments;
use crate::semantic_lists::LITERAL_CLASSES;
use crate::{FxMap, UserFn};
use ry_core::ast::{Arg, Expr};
use ry_core::types::{ClassVector, RType};
use ry_typeshed::ParamSpec;

/// Every S4 generic and method registration the project makes.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct S4Inventory {
    generics: FxMap<String, S4Generic>,
    /// Some registration may not mean what it spells (a computed generic
    /// name, or a masked registration helper).
    all_incomplete: bool,
    /// A registration with a computed generic name may belong to any
    /// generic, including one with no recorded entry.
    computed_name: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct S4Generic {
    decl: GenericDecl,
    /// In registration order: a later identical signature replaces an earlier one.
    methods: Vec<S4Method>,
    /// Some registration's signature could not be interpreted.
    incomplete: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum GenericDecl {
    /// No recognized `setGeneric` with a definition: the implicit generic of
    /// an existing function.
    #[default]
    Absent,
    /// `setGeneric` with a literal forwarding definition. `signature` is the
    /// declared dispatch order, every entry a formal other than `...`.
    Literal {
        formals: Vec<String>,
        signature: Vec<String>,
    },
    /// A declaration whose formals, signature, or result are not established.
    Opaque,
}

#[derive(Clone, Debug, PartialEq)]
struct S4Method {
    /// Authored entries, all positional or all named by formal.
    entries: Vec<(Option<String>, String)>,
    /// The definition's binding in [`Definitions`]; `None` for a definition
    /// that is not a function literal.
    function: Option<String>,
}

/// One call of a possible generic, after aliasing.
pub(crate) struct S4Call<'a> {
    pub(crate) head: Callee<'a>,
    pub(crate) own_package: Option<&'a str>,
    pub(crate) args: &'a [Arg],
    pub(crate) arg_types: &'a [RType],
    /// Per actual: the expression is a `new()` call whose head is proven to
    /// be `methods::new`, the only producer of an exactly known class.
    pub(crate) instances: &'a [bool],
    /// The formals of the base function of this name, if any.
    pub(crate) core_formals: Option<&'a [ParamSpec]>,
}

pub(crate) enum S4Selection<'a> {
    /// R provably dispatches to this method.
    Method(&'a UserFn),
    /// A project method may answer the call; its result is unknown.
    Uncertain,
    /// No project method can be shown to apply: later stages decide.
    NoCandidate,
}

impl S4Inventory {
    pub(crate) fn declare_generic(&mut self, name: &str, decl: GenericDecl) {
        let generic = self.generics.entry(name.to_string()).or_default();
        generic.decl = merge_decl(&generic.decl, &decl);
    }

    /// Record a `setMethod`; `entries` is `None` when its signature could
    /// not be interpreted.
    pub(crate) fn register_method(
        &mut self,
        generic: &str,
        entries: Option<Vec<(Option<String>, String)>>,
        function: Option<String>,
    ) {
        let generic = self.generics.entry(generic.to_string()).or_default();
        match entries {
            Some(entries) => generic.methods.push(S4Method { entries, function }),
            None => generic.incomplete = true,
        }
    }

    /// Whether `new("A")` returns an instance of exactly `A`: a project
    /// `initialize` method may return any object.
    fn instances_exact(&self) -> bool {
        !self.defines_method(|generic| generic == "initialize")
    }

    /// Record a `setMethod`/`setGeneric` whose generic name is computed.
    pub(crate) fn mark_computed_name(&mut self) {
        self.computed_name = true;
        self.all_incomplete = true;
    }

    pub(crate) fn mark_all_incomplete(&mut self) {
        self.all_incomplete = true;
    }

    /// Whether the project registers a method, interpretable or not, for a
    /// generic `generic` accepts.
    pub(crate) fn defines_method(&self, generic: impl Fn(&str) -> bool) -> bool {
        self.all_incomplete
            || self.generics.iter().any(|(name, entry)| {
                (entry.incomplete || !entry.methods.is_empty()) && generic(name)
            })
    }

    /// Merge a later file's registrations after this table's.
    pub(crate) fn append(&mut self, later: &S4Inventory) {
        self.all_incomplete |= later.all_incomplete;
        self.computed_name |= later.computed_name;
        for (name, entry) in &later.generics {
            let generic = self.generics.entry(name.clone()).or_default();
            generic.decl = merge_decl(&generic.decl, &entry.decl);
            generic.methods.extend(entry.methods.iter().cloned());
            generic.incomplete |= entry.incomplete;
        }
    }
}

fn merge_decl(earlier: &GenericDecl, later: &GenericDecl) -> GenericDecl {
    match (earlier, later) {
        (GenericDecl::Absent, decl) | (decl, GenericDecl::Absent) => decl.clone(),
        (earlier, later) if earlier == later => earlier.clone(),
        _ => GenericDecl::Opaque,
    }
}

impl S4Generic {
    /// The complete formals and the dispatch signature, when established.
    fn established(&self, core_formals: Option<Vec<String>>) -> Option<(Vec<String>, Vec<String>)> {
        let (formals, signature) = match &self.decl {
            GenericDecl::Literal { formals, signature } => (formals.clone(), signature.clone()),
            GenericDecl::Absent => {
                let formals = core_formals?;
                if formals.first().is_some_and(|formal| formal == "...") {
                    return None;
                }
                let signature = formals.iter().filter(|f| *f != "...").cloned().collect();
                (formals, signature)
            }
            GenericDecl::Opaque => return None,
        };
        // A generic whose only formal is `...` dispatches on `...`.
        (!signature.is_empty()).then_some((formals, signature))
    }

    /// Each method as one class per dispatch position, `ANY`-padded, the
    /// last registration of a signature winning. `None` when a registration
    /// does not map onto the signature or mixes named and positional entries.
    fn normalized(&self, signature: &[String]) -> Option<Vec<(Vec<&str>, Option<&str>)>> {
        let mut methods: Vec<(Vec<&str>, Option<&str>)> = Vec::new();
        for method in &self.methods {
            let named = method
                .entries
                .iter()
                .filter(|(name, _)| name.is_some())
                .count();
            if named != 0 && named != method.entries.len() {
                return None;
            }
            let mut classes = vec!["ANY"; signature.len()];
            for (index, (name, class)) in method.entries.iter().enumerate() {
                let position = match name {
                    Some(name) => signature.iter().position(|formal| formal == name)?,
                    None => index,
                };
                *classes.get_mut(position)? = class.as_str();
            }
            methods.retain(|(earlier, _)| *earlier != classes);
            methods.push((classes, method.function.as_deref()));
        }
        Some(methods)
    }
}

impl Definitions {
    pub(crate) fn s4(&self) -> &S4Inventory {
        &self.s4
    }

    pub(crate) fn s4_mut(&mut self) -> &mut S4Inventory {
        &mut self.s4
    }

    /// The project S4 method a call provably dispatches to.
    pub(crate) fn select_s4(&self, call: S4Call<'_>) -> S4Selection<'_> {
        let Some(generic) = self
            .s4
            .generics
            .get(call.head.member())
            .filter(|generic| generic.incomplete || !generic.methods.is_empty())
        else {
            // A computed registration may have made this any generic with a
            // method for any actual's class.
            let classed = call.instances.contains(&true)
                || call
                    .arg_types
                    .iter()
                    .any(|actual| actual.class.has_known_class());
            return if self.s4.computed_name && classed {
                S4Selection::Uncertain
            } else {
                S4Selection::NoCandidate
            };
        };
        // Fall through when no method names any actual's explicit class. An uninterpreted registration may
        // name any of them.
        let named = |class: &str| {
            call.arg_types
                .iter()
                .any(|actual| actual.class.first().is_some_and(|first| *first == *class))
        };
        let classed = call
            .arg_types
            .iter()
            .any(|actual| actual.class.first().is_some());
        if !((generic.incomplete || self.s4.all_incomplete) && classed)
            && !generic
                .methods
                .iter()
                .any(|method| method.entries.iter().any(|(_, class)| named(class)))
        {
            return S4Selection::NoCandidate;
        }
        if let Callee::Qualified { package, .. } = call.head
            && call.own_package != Some(package)
        {
            // A project `setGeneric` makes another package's function a
            // different object; methods on an existing function may reach it.
            return match generic.decl {
                GenericDecl::Literal { .. } => S4Selection::NoCandidate,
                _ => S4Selection::Uncertain,
            };
        }
        if self.s4.all_incomplete || generic.incomplete {
            return S4Selection::Uncertain;
        }
        let Some((formals, signature)) = generic.established(call.core_formals.map(param_names))
        else {
            return S4Selection::Uncertain;
        };
        let (Some(methods), Some(classes)) = (
            generic.normalized(&signature),
            dispatch_classes(&formals, &signature, &call, self.s4.instances_exact()),
        ) else {
            return S4Selection::Uncertain;
        };
        let exact = classes.iter().all(Option::is_some).then(|| {
            methods.iter().find(|(method, _)| {
                method
                    .iter()
                    .zip(&classes)
                    .all(|(entry, class)| class.as_deref() == Some(*entry))
            })
        });
        let receiver_only = || {
            let receiver = classes[0].as_deref()?;
            methods
                .iter()
                .all(|(method, _)| method[1..].iter().all(|entry| *entry == "ANY"))
                .then(|| methods.iter().find(|(method, _)| method[0] == receiver))
                .flatten()
        };
        match exact.flatten().or_else(receiver_only) {
            Some((_, Some(function))) => self
                .live
                .get(*function)
                .map_or(S4Selection::Uncertain, S4Selection::Method),
            _ => S4Selection::Uncertain,
        }
    }

    /// The return slots of every recorded method definition.
    pub(crate) fn s4_method_slots(&self) -> impl Iterator<Item = usize> + '_ {
        self.s4
            .generics
            .values()
            .flat_map(|generic| &generic.methods)
            .filter_map(|method| self.live.get(method.function.as_deref()?))
            .map(|function| function.return_slot)
    }

    /// Seed each method's dispatch formals with the classes its signature
    /// names, as lower bounds: a formal is a binding, never exact evidence.
    pub(crate) fn seed_s4_parameters<'p>(
        &mut self,
        core_formals: impl Fn(&str) -> Option<&'p [ParamSpec]>,
    ) {
        let mut seeds = Vec::new();
        for (name, generic) in &self.s4.generics {
            let Some((formals, signature)) =
                generic.established(core_formals(name).map(param_names))
            else {
                continue;
            };
            for (classes, function) in generic.normalized(&signature).unwrap_or_default() {
                let Some(function) = function else {
                    continue;
                };
                for (formal, class) in signature.iter().zip(classes) {
                    if class != "ANY" && class != "missing" {
                        let index = formals.iter().position(|f| f == formal);
                        seeds.push((
                            function.to_string(),
                            index,
                            formal.clone(),
                            class.to_string(),
                        ));
                    }
                }
            }
        }
        for (function, index, formal, class) in seeds {
            let Some(parameter) = self
                .live
                .get_mut(&function)
                .and_then(|function| function.params.get_mut(index?))
            else {
                continue;
            };
            // The formal receives a supplied actual, never its default. R
            // conforms a method's formals to the generic's; seed a class
            // only where the names agree, and never a literal's implicit one.
            parameter.type_ =
                if parameter.name == formal && !LITERAL_CLASSES.contains(&class.as_str()) {
                    RType::unknown().with_class(ClassVector::guard(&class))
                } else {
                    RType::unknown()
                };
        }
    }
}

fn param_names(params: &[ParamSpec]) -> Vec<String> {
    params.iter().map(|param| param.name.clone()).collect()
}

/// The class each dispatch position receives: `"missing"` for an omitted
/// formal, `None` when not known exactly. `None` overall when the actuals
/// cannot be matched to the generic's formals exactly.
fn dispatch_classes(
    formals: &[String],
    signature: &[String],
    call: &S4Call<'_>,
    instances_exact: bool,
) -> Option<Vec<Option<String>>> {
    let (args, arg_types) = (call.args, call.arg_types);
    if args.iter().any(|argument| {
        matches!(&argument.value, Expr::Missing(_))
            || matches!(&argument.value, Expr::Ident { name, .. } if name == "...")
    }) {
        return None;
    }
    let names: Vec<&str> = formals.iter().map(String::as_str).collect();
    let matched = match_arguments(&names, args);
    if !matched.unmatched_named.is_empty() {
        return None;
    }
    let classes = signature.iter().enumerate().map(|(position, formal)| {
        let index = formals.iter().position(|f| f == formal)?;
        let Some(actual) = matched.arg_for_param(index) else {
            return Some("missing".to_string());
        };
        // Only a `new("A")` call itself proves an exact class. Other
        // constructors and class attributes can name a superclass of the
        // dispatched one (`factor(ordered = TRUE)` is an `ordered`), and a
        // binding may have been replaced or modified since it was read.
        let class = &arg_types.get(actual)?.class;
        if instances_exact && call.instances.get(actual) == Some(&true) && class.has_known_class() {
            return class.first().map(|first| first.to_string());
        }
        // Only a plain literal proves its class is the implicit one: a
        // `dim` attribute alone makes a double dispatch as `matrix`. The
        // receiver keeps explicit classes only.
        let literal = match &args[actual].value {
            Expr::Double(..) => 0,
            Expr::Integer(..) => 1,
            Expr::String(..) => 2,
            Expr::Logical(..) => 3,
            _ => return None,
        };
        (position > 0).then(|| LITERAL_CLASSES[literal].to_string())
    });
    Some(classes.collect())
}
