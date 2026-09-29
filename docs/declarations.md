# Authored type declarations

[Getting started](../README.md) · [Structured facts](facts.md) · [Type summaries](types.md)

ry keeps authored constraints separate from inferred `RType` facts. A type at
scope exit describes one analysis snapshot. A declaration describes a
provider's claim about a function, with its original source and attachment.
Neither a compact hint nor an inferred fact silently becomes a contract.

This document describes the declaration core in [#592] and shared checking in
[#593]. The first external reader and public `dump-facts` annotation export
belong to [#594]. Until then, `dump-facts` keeps schema versions 1 and 2 and
has no annotation flag or empty annotation placeholder.

## Supported canonical vocabulary

The initial `TypeExpr` language supports exact R storage modes `logical`,
`integer`, `double`, `complex`, `character`, `raw`, `list`, and `null`. It does
not treat `numeric` as an alias: an adapter must establish its provider's
predicate semantics first. `unknown` records an explicit lack of a type claim;
it is not a request to skip checking. `none` in a signature means no
constraint was supplied, distinct from an explicit `unknown` constraint.

An atomic constraint can omit length, specify an exact nonnegative length, or
specify `1+` for nonempty. `null` can only have length zero. Unions contain
supported alternatives; canonical formatting flattens nested unions, then
sorts and deduplicates their alternatives.

```text
integer
integer<len=1>
list<len=1+>
union[character, integer<len=1>]
unknown
```

A signature includes assignment behavior, parameter identity, ordinary or
variadic form, supplied/defaulted status, evaluation behavior, and an optional
return constraint. Names use JSON string quoting. A variadic formal must be
named `...` and has unknown supplied status. The canonical spelling is
parseable and normalizes whitespace and union order:

```text
fn[entry_only](required/promise "x": integer, variadic/unknown "...": none) -> none
```

Assignment behavior is `entry_only`, `persistent_binding`, `coerces_input`,
or `unknown`. Evaluation behavior is `value`, `promise`, `quoted`, or
`unknown`. These are metadata about a provider's semantics, not claims that
ry implements the corresponding runtime behavior. Unsupported effects must
remain explicit rather than being translated into a plain entry condition.

Parsing and formatting cap a declaration at 4096 bytes, 16 type nesting
levels, 64 type nodes, 16 union alternatives, 64 parameters, and 256 bytes per
parameter name. The node limit applies to the original tree before union
flattening. Each union allows at most 16 direct members, and the flattened,
distinct union also allows at most 16 alternatives.
Exceeding a limit returns a resource-limit error. Invalid
syntax is a separate error; neither becomes an accepted `unknown` type.

## Provenance and translation

Each record retains provider and version, source path/span/raw text, and a
target identified by local source definition or qualified package function.
Translation status is independent of whether the user adopted the claim:

| Translation | Meaning |
| --- | --- |
| `exact` | The supported signature represents the entire audited claim. |
| `partial` | A supported necessary part exists; unsupported residual text and spans remain. |
| `unsupported` | No sound supported constraint can be extracted. |
| `invalid_syntax` | The provider's expected syntax was malformed. |
| `ambiguous_attachment` | The target cannot be identified safely. |

Evidence use separately records `adopted_contract`, `runtime_guard`, or
`documentation_candidate`. Recognition alone establishes none of these as a
runtime guard. The shared checker keeps an unresolved conflict visible rather
than selecting a declaration by file order.

## Shared checking boundary

`Checker::set_declaration_records` and `Project::set_declaration_records`
accept structured records. A source reader must verify each local target
against the source AST and current source text before it installs records.
A matching display name alone does not identify a function. Readers must
reinstall records after an annotation-only edit or a configuration change.
Project rechecks affected files when its record set changes.

Only an explicitly adopted, exact `entry_only` signature can supply a body
entry type. Its declared parameters must be an ordered subset of the R formals,
with matching supplied/defaulted status. Partial, unsupported,
invalid, and ambiguous records retain their status but supply no entry type.
Equivalent adopted signatures share provenance. Conflicting adopted signatures
block adoption. A declaration of more than four union alternatives remains
checkable at a known call but exceeds the body inference cap; the checker
reports this limit and leaves the body entry type unknown.

The checker keeps a defaulted formal's default marker when it applies a
declared entry type. Later assignments update the local type in the usual
way. Call checks use R's exact-name, unambiguous partial-name, and positional
argument matching. They skip omitted arguments and unknown evidence. A mixed
inferred union cannot prove a mismatch. Return checks use independently
inferred returns; a declaration does not verify itself.

Known-call mismatch checks require a current callable identity. A local
assignment later in a body does not prove that an earlier read used that
binding, and a historical function definition does not prove a call head still
resolves to it after removal or active-binding installation. When an evaluated
read, call, or operator can change a binding and its effects cannot be proved
absent, later call checks become inconclusive; source attachment, default
checks, and independent return evidence remain available. A typeshed
signature alone does not establish that a call is effect-free.
For precision, even an ordinary local data read or subscript can make a later
contract check inconclusive when the checker cannot prove its binding effects
absent. Local assignments inside an immediately invoked closure belong to
that closure; they cannot certify a later read in its caller.

`Checker::declaration_findings` and `Project::declaration_findings` expose
structured mismatch, partial, unsupported, conflict, invalid-syntax, and
ambiguous-attachment findings. Parse or encoding errors suppress findings
from a repaired source tree. These findings are separate from public RY rule
codes; [#594] maps them after a real source reader supplies records.

## Export boundary

`convert_inferred` returns `exact`, `proposed` with reasons, or `refused` with
reasons. Exact means faithful to the *available inferred fact*, not proof of
an author's intended input domain. Scope-exit values become proposals;
default-derived parameter types and scope-exit function-entry/return evidence
are refused. Partial callable signatures, unknown storage modes, and possibly
truncated class vectors are refused. Unknown class facts, represented classes,
and schemas outside the initial vocabulary require an explicit proposal or
refusal, with uncertainty retained.

Conversion inspects at most 64 inferred type nodes and checks the final
constraint against the canonical grammar before reporting success. A narrowed
union whose outer length is not carried by its members becomes a proposal
with that loss stated explicitly. Very wide schemas are not scanned in full;
their field identity remains unverified in the proposal.

The annotation serializer is tested with populated exact/partial/unsupported
records and validates same-file source/residual spans. [#594] will wire real
adopted source records into a public schema-3 `dump-facts` option, include
translation and evidence status per record, and keep the existing
`declaration` field's source-definition meaning unchanged. Schema 1 and 2
consumers need no migration until they opt into that option.

[#592]: https://github.com/sims1253/ry/issues/592
[#593]: https://github.com/sims1253/ry/issues/593
[#594]: https://github.com/sims1253/ry/issues/594
