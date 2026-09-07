# ry parser dependency

This is tree-sitter-r 1.3.0 from crates.io, corresponding to upstream commit
`f2289db204dda2bf7c7ea720de5c2a2c3eff2530`. The upstream MIT license is retained
in LICENSE. Only files required by the Rust binding and grammar regeneration
are included.

R accepts spaces, newlines, and comments between the closing brackets of a
`[[` subset. Upstream deliberately treats `]]` as one token; see its README.
ry needs the R spelling because a parse error can suppress otherwise useful
file diagnostics.

Local changes:

- grammar.js represents the closing pair as two `]` tokens.
- src/scanner.c changes the double-bracket scope to a single-bracket scope
  after consuming the first closing token. The existing scanner handles the
  second token, whitespace, newlines, comments, and serialized scope state.
- queries/highlights.scm removes the obsolete `]]` token from its pattern.
- src/parser.c, src/grammar.json, and src/node-types.json are regenerated.

Regenerate with tree-sitter CLI **0.24.7** (language ABI 14):

```sh
cargo install tree-sitter-cli --version 0.24.7 --locked
scripts/regenerate-r-parser.sh
```

Normal Cargo builds compile the checked-in C files and need no generator or
network access beyond ordinary Cargo dependencies. Parser and checker tests
cover separated tokens, comments, UTF-8 locations, incremental edits, malformed
brackets, and retained diagnostics; the R oracle supplies runtime controls.
