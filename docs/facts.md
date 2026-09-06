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
`--position` option and no type-at-reference or resolved-symbol contract.

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
while the binding's `name` contains the decoded name. `start` and `end`
are one-based `[line, column]` pairs. Columns count Unicode scalar values;
a tab counts as one character, not a display-width expansion. Lines advance
at LF, so CRLF bytes remain part of the source and byte ranges.

Function scope spans cover the function literal when the original AST has
that site. The top-level span covers the file. A scope synthesized during
analysis can have `span: null`. No span establishes a resolved symbol identity.

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
