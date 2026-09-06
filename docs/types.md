# Inferred types

[Getting started](../README.md) · [Usage](usage.md) · [Configuration](configuration.md)

`ry dump-types` prints names and inferred types as JSON on stdout. It uses
the same analysis and package context as `ry check`, and the same type
strings as editor inlay hints. The output groups bindings by lexical scope
(the top level or a function body), so tools can query several positions
without running the checker again.

## Example

Save this as `types.R`:

```r
offset <- 1L
add_offset <- function(x = 2L) {
  result <- x + offset
  result
}
```

```sh
ry dump-types types.R
ry dump-types types.R --position 4:3
```

The second command returns the `add_offset` scope. Its `bindings` array
includes this entry:

```json
{"name": "offset", "kind": "closed-over", "type": "integer<len=1>", "start": [1, 1]}
```

## Output fields

Positions are 1-based `[row, column]` pairs; columns count characters,
not bytes. Scopes are ordered by start position, bindings by name.
`unknown` marks bindings whose types ry could not infer. It does not cause
the command to fail.

Binding kinds:

- `param`, a formal of this scope that the body never reassigns. A
  reassigned formal degrades to `local` at its reassignment site
  (R rebinds rather than narrows).
- `local`, first assigned inside this scope's own body (assignments in
  `if` / `for` / `while` bodies and braced value blocks count; they bind
  in the enclosing function in R).
- `closed-over`, function scopes only: present because the body's
  scope is cloned from the enclosing one at the point of definition.
- `imported`, top-level bindings the file never assigns, supplied by
  the host environment (for example Shiny server fragments, where
  `input` / `output` / `session` are ambient).

Each binding's `start` points at its definition site, the formal, the
first assignment, or, for `closed-over`, the site in the nearest
enclosing scope that defines the name (`null` when none is recorded).
For a reassigned local, this first assignment need not define its final value.

## Querying a position

`--position LINE:COL` (repeatable) selects the innermost recorded scope
containing each position and drops locals whose first assignment follows it.
Types still describe the scope at the end of its body. The option does not
return the type at a reference or the flow state at the selected position.

Save this as `reassigned.R`:

```r
x <- 1L
y <- x
x <- "later"
```

```sh
ry dump-types reassigned.R --position 2:6
```

The query reports `x` as `character<len=1>` with `start: [1, 1]`, even though
`x` is an integer when R evaluates `y <- x`. The type comes from the final
assignment; `start` points to the first assignment. Do not use this pair as
evidence of the type or defining assignment at the selected reference.

## Files and project context

A directory argument expands to every discoverable R file under it,
using `ry check`'s discovery rules, including the discovered `ry.toml`'s
`exclude` patterns. `--project-root <DIR>` overrides the analysis root
for non-package files; by default each file is analyzed in the context
of its nearest enclosing package (the ancestor directory with a
`DESCRIPTION`), else the directory owning the discovered `ry.toml`, else
the working directory, mirroring `ry check`'s per-package grouping. The
exit code is 0 even when the analyzed code has
diagnostics; it is non-zero only for usage, IO, or internal failure.

## Scope limits

Scopes reflect the checker's snapshot semantics: each table is the
scope's state at the end of its body, and a nested function captures the
enclosing scope as of its definition point (ry's documented closure
approximation). Anonymous function literals used as call arguments are
inferred in discarding mode and are therefore not recorded as scopes,
but named functions defined *inside* such a callback do complete and are
recorded, so a dump can contain a scope whose enclosing scope is absent.
