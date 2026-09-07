# Structured analysis facts

[Getting started](../README.md) · [Usage](usage.md) · [Configuration](configuration.md)

`ry dump-facts` exports versioned JSON for tools that need structured types and
analysis context. It uses the same configuration, file discovery, package
resolution, and scope capture as `ry dump-types`. It does not run the analyzed
R code or load its packages through R.

```sh
ry dump-facts R/ --format json > facts.json
```

Use `--project-root DIR` to set the analysis root for files outside an R package.
Each package still uses its nearest enclosing `DESCRIPTION` directory. The
first input anchors `ry.toml` discovery, as it does for `dump-types`.

## Scope snapshots

Every exported scope and binding has `snapshot_kind: "scope_exit"`. Types
describe the state at the end of the recorded scope body. There is no
`--position` option. Schema 1 has no type-at-reference or resolved-symbol
contract. Add `--references` for the bounded reference facts in schema 2.

For example:

```r
x <- 1L
y <- x
x <- "later"
```

The top-level snapshot describes `x` as character and `y` as integer. The
`declaration` for `x` points to its first assignment, `x <- 1L`. This span does
not establish which assignment produced its final value, or what type `x`
had when R evaluated `y <- x`.

The checker analyzes nested functions using a copy of the enclosing analysis
scope. That scope can include project facts collected before the body is walked.
Anonymous function arguments are usually analyzed without recording their
scopes. Named functions inside those arguments can still have records. A
missing record means that ry did not export that fact; it does not prove that
a scope or binding does not exist.

## Reference facts (schema version 2)

```sh
ry dump-facts R/ --references > facts.json
```

`--references` selects schema 2. Without this flag, `dump-facts` emits schema 1.
`dump-types` keeps its own output format. Reference capture runs alongside
scope capture in the same project check, without changing diagnostic inference.

The `reference_facts` capability is `"same_file_ordered_prefix"`. Consumers
that accepted only `"same_file_straight_line"` must opt into this broader
contract. The schema version remains 2; IDs are still local to one file and
analysis snapshot, and coverage remains partial.

Schema 2 keeps the scope and binding snapshots below. At the root it replaces
`snapshot_kind` with `scope_snapshot_kind: "scope_exit"` and adds:

```json
"capabilities": {
  "scope_snapshots": true,
  "reference_facts": "same_file_ordered_prefix",
  "reference_coverage": "partial"
}
```

Each file also has `definitions` and `references`. A definition contains `id`,
`name`, `kind` (`"assignment"` or `"formal"`), `span`, and `scope_span`.
The span identifies the source definition; the scope span identifies its
lexical scope. Reference and definition names retain the checker's raw
spelling, including backticks; these are not canonical R symbol names.
Use the reference-to-definition ID for identity, not a join by name.
IDs are local to this file and analysis snapshot. Join them
only within the same file, source hash, and context ID. Unchanged inputs
produce deterministic output. IDs carry no identity outside that snapshot;
do not reuse them after its source hash or context ID changes.

Each reference contains:

| Field | Meaning |
| --- | --- |
| `name`, `span` | The read name and its source token. |
| `snapshot_kind` | `"reference"`; evidence at this read, not scope exit. |
| `resolution_status` | `"resolved"`, `"ambiguous"`, `"unresolved"`, or `"unsupported"`. |
| `definition_id` | An ID in this file's `definitions`, or `null`. |
| `type_at_reference` | A structured type, or `null` when resolution is not established. |
| `reason` | The checker's explanation for missing evidence, or `null`. |

The checker supports a conservative subset of same-file code: literal
assignments, copies of established bindings, and a function's own formals and
locals. Ordinary literal/copy reassignments to non-formal locals get a distinct
definition ID for each write. A read uses the definition installed at that
point, after the preceding assignment's RHS has been analyzed. A copy gets its
own definition ID and keeps the source binding's established type.

A formal has unknown type even when it has a default. R callers can supply a different value. Reading a formal can force
caller or default code that changes the frame. After that read, the checker
discards binding identities for the rest of the scope, including nested
functions analyzed from it. A fresh assignment cannot restore that evidence.
The formal read itself may resolve with unknown type, but a copy made from
that read and later reads remain unsupported. Reads without an established
own-scope assignment value, including outer, external, and unresolved names,
apply the same barrier.

For example, in `x <- 1L; y <- x; y`, the read of `x` resolves to its assignment
with integer type. The read of `y` resolves to the copy assignment with the
same type.

The supported subset assumes ordinary initial bindings. It does not certify
behavior under arbitrary ambient active bindings. Consistently spelled
backtick names without escapes are supported. Unquoted namespace-qualified
names, dots arguments, escaped names, syntax-primitive names, and reassignment
through equivalent plain/backtick spellings are excluded. A differently
spelled read cannot resolve by matching a decoded name.

Calls, operators, indexing, control flow, nonstandard evaluation, and other
unsupported statements end the supported prefix. The entire statement and
its suffix remain unsupported, including fresh literal assignments. Earlier
proven reads retain their definition IDs and types. This does not establish
evaluation order inside an unsupported expression. Top-level braces that the
parser flattens into a statement sequence follow that sequence.

Mixed equivalent spellings, writes to formals, and unsupported declaration
names still exclude the whole lexical scope. Nested function bodies remain
unsupported if their enclosing scope contains an opaque statement, even after
the function's definition: those bodies can run later. Default expressions and
reads captured from enclosing scopes are unsupported. Unresolved, ambiguous,
and unsupported records have no definition ID or type. A missing record is also
unavailable evidence: the export does not promise to enumerate every possible
runtime read. Check the status of each record. A concrete scope-exit type
cannot fill a gap in reference evidence, and a resolved reference alone does
not establish that a rename or other edit is safe.

See the [fixed coverage panel](corpus/reference-prefixes.md) for measured
changes and unchanged real-source cases.

## Schema version 1

The top-level JSON object contains:

| Field | Meaning |
| --- | --- |
| `schema_version` | Integer `1`. Consumers must check this before reading facts. |
| `producer` | ry `version` and `executable_hash`. |
| `snapshot_kind` | `"scope_exit"`. |
| `coordinates` | Encoding and position conventions below. |
| `configuration` | Effective config, its root, and environment profile roots. |
| `contexts` | Per-project analysis identities and their inputs. |
| `discovery` | Whether size/depth limits omitted input, with truncation details. |
| `files` | Analyzed files, their source hashes, context IDs, and scopes. |

Consumers must treat absent fields as unavailable evidence. `null` represents
an unavailable optional value, such as an unrecorded declaration span.
An empty array is a known empty collection only when its enclosing object's
`kind` establishes that knowledge.

### Files and scopes

A file has `path` (an absolute canonical path), `source_hash`, `context_id`,
`scopes`, and `imports`. `imports` maps names to package names from static
workspace resolution. These names need not appear in captured lexical bindings;
the mapping does not establish resolution at a reference. The source hash covers the exact UTF-8 file bytes, including
line endings. ry rejects a non-UTF-8 file instead of returning byte offsets
into transcoded text. After removing identical input paths, it rejects distinct paths that resolve
to the same canonical file.

A scope has `kind` (`"top"` or `"function"`), `name` (string or `null`),
`snapshot_kind`, `span`, `data_mask_unknown`, `search_path_unknown`, and
`bindings`. The uncertainty flags come directly from the checker. They can
be true even when some bindings have concrete types. They do not enumerate
all sources of uncertainty in R.

A binding has:

| Field | Meaning |
| --- | --- |
| `name` | The binding's name in this scope snapshot. |
| `kind` | `"param"`, `"local"`, or `"unclassified"`. |
| `snapshot_kind` | `"scope_exit"`. |
| `type` | The structured type below. |
| `declaration` | A source site and its role, when available. |
| `origin` | Raw checker alias and derivation metadata. |

`param` identifies an own formal that still has the checker's parameter marker.
`local` identifies a name with a syntactic assignment site in this scope's AST.
These categories describe recorded metadata, not proof that an assignment
executes or supplies the final value. An AST walker can see assignments in
quoted or defused code.

Other bindings are `unclassified`. The scope model does not record enough
provenance to distinguish captured names, runtime bindings, and names inserted
by dynamic operations such as `assign()`. A name without an own syntactic site
therefore has no declaration span, even if an enclosing scope or package import
has the same name. Static package imports remain available in the file's
`imports` map.

`declaration.kind` is `"formal"`, `"first_assignment"`, or `"unavailable"`.
`declaration.span` is a span or `null`. `first_assignment` means the first
syntactic assignment in this scope, not the first executed assignment.
Synthetic, missing, empty, or invalid declaration spans become `null`.
`declaration.defines_final_value` is always `"not_established"`.

`origin` contains:

- `callee_alias`: raw checker metadata with `target` (a name) and
  `resolution: "not_established"`, or `null`.
- `list_derived`: whether the checker marks the binding as list-derived.
- `default_parameter_derived`: whether its type came from a parameter default.

These fields are snapshot metadata. Callee-alias metadata can remain on a
binding with a non-function type, such as an integer copied from a name that
shadows a package function. It does not establish that a value is callable,
that the name resolves to a particular definition, or that a rename is safe.

### Types

Each type is an object with `kind: "r_type"` and the following fields:

| Field | Shape |
| --- | --- |
| `mode` | `logical`, `integer`, `double`, `complex`, `character`, `raw`, `list`, `null`, `function`, `opaque`, or `union`. |
| `length` | `{"kind":"known","value":N}` or `{"kind":"unknown"}`. |
| `class` | `kind` (`known` or `unknown`), ordered `names`, `capacity: 4`, and `may_be_truncated`. |
| `columns` | `{"kind":"unknown"}` or an object with `kind` (`complete` or `partial`), `entries`, and `locally_constructed`. |
| `members` | `{"kind":"not_applicable"}`, `{"kind":"unknown"}`, or `{"kind":"known","types":[...]}`. |
| `function` | `{"kind":"not_applicable"}`, `{"kind":"unknown"}`, or the partial signature below. |

`opaque` means the storage mode is unknown; other fields may still carry
information. A known empty class list means no explicit class. The checker
stores at most four class names. At capacity, `may_be_truncated` is true
because it cannot establish whether the original vector was longer.

Column entries have `key` and a recursive `type`. Order and duplicate keys
are preserved. A key is the checker's schema key, not necessarily a literal
field name. For unnamed list elements, the checker uses synthetic keys such
as `[[1]]`. A literal field named `[[1]]` can have the same key; the type model
does not retain enough provenance to distinguish these cases. Do not use a
schema key alone as evidence for a named-field edit. `complete` with `entries: []` is a known empty schema;
`partial` with `entries: []` establishes no columns but allows others;
`unknown` has no schema evidence. `locally_constructed` is the checker's
construction marker, not a guarantee that later operations cannot change it.

Union `members.types` contains recursively structured alternatives. A function
with a recorded signature has:

```json
{
  "kind": "partial",
  "params": [],
  "parameters_complete": false,
  "return_type": {
    "kind": "r_type",
    "mode": "integer",
    "length": {"kind": "known", "value": 1},
    "class": {"kind": "known", "names": [], "capacity": 4, "may_be_truncated": false},
    "columns": {"kind": "unknown"},
    "members": {"kind": "not_applicable"},
    "function": {"kind": "not_applicable"}
  }
}
```

`params` holds ordered parameter types where available. It does not encode a
complete named-formal contract, defaults, or R argument matching. An empty
`params` array does not establish that a function accepts no arguments.
The export has no NA facts because the checker does not model them.

### Source spans

A source-backed span has this shape:

```json
{"bytes": [0, 2], "start": [1, 1], "end": [1, 2]}
```

`bytes` is a zero-based, half-open UTF-8 byte range. Slice the original source
as `source[start_byte:end_byte]` after checking its hash. Declaration spans
cover the raw source token: a backtick-quoted name includes its backticks,
while `name` retains the checker's spelling. `start` and `end`
are one-based `[line, column]` pairs. Columns count Unicode scalar values;
a tab counts as one character, not a display-width expansion. Lines advance
at LF, so CRLF bytes remain part of the source and byte ranges.

Function scope spans cover the function literal when the original AST has
that site. The top-level span covers the file. A scope synthesized during
analysis can have `span: null`. A span alone establishes no resolved symbol
identity; schema 2 uses explicit reference-to-definition IDs for that evidence.

### Cache identities and ordering

All hashes use SHA-256 and have a `sha256:` prefix. `producer.executable_hash`
covers the running binary, so rebuilding ry can invalidate facts without a
version change. It also identifies the embedded typeshed contents.

Each context has an `id` and an `inputs` object. The ID hashes the compact JSON
encoding of `inputs`, with sorted object keys. Inputs include:

- the canonical resolution root and producer identity;
- the hash of effective configuration, including environment profile roots;
- bundled typeshed source metadata, its build identity, and a hash of all
  successfully loaded custom stub content after overrides;
- canonical paths and source hashes for every analyzed file in the group;
- a hash of the resolved workspace maps and sets supplied to the checker,
  including imports, attached packages, serialized bindings, and S3 methods;
- effective built-in environment bindings for every analyzed file, including
  Shiny classification triggered by unselected `app.R`, `server.R`, or `ui.R`;
- degraded-scope reports from workspace resolution.

Use the file path, source hash, and context ID together. A change to another
file, a custom stub, or static package metadata can change a file's facts
without changing its own source hash. Paths are local identities, not IDs
that remain stable when a project moves. These hashes describe the resolved
inputs for this run; they are not a filesystem watcher or a promise that R's
runtime environment matches the static model.

References and definitions sort by byte start and end. Definition IDs retain
the checker's allocation order, so IDs need not increase through the array.
Files sort by canonical path; contexts by ID; scopes by byte start, byte end,
then kind; bindings by name. JSON object keys sort lexically. Union alternatives
sort by their compact structured JSON. Class names, columns, and parameter
types retain their meaningful order. No timestamps appear in the output.

## Failures and incomplete discovery

Diagnostics do not fail the export. Invalid options, missing/unreadable files,
parse failures, non-UTF-8 input or paths, and invalid configuration return a nonzero
status without emitting facts. Malformed custom stubs follow the shared
loader's policy: warn on stderr and use the valid stubs. Context identity
covers the stubs actually used.

Size and depth limits set `discovery.complete` to false and list the omitted
files or directories. Config exclusions and normal package discovery rules
still apply. Hitting `index.max-files` fails the export because filesystem
traversal order cannot establish a deterministic subset. Narrow the input or
raise the limit. Check discovery completeness before treating an export as a
census of the requested project.
