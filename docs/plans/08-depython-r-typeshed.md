# Plan 08: Remove Python tooling from r-typeshed; validate via `ry typeshed validate`

## Status: run immediately after plan 04 completes. Blocks plan 05.
## Repos: ry (this repo) + r-typeshed (/home/m0hawk/Documents/r-typeshed, write access).

## Context / decision

Plan 02 introduced Python scripts (`validate.py`, `verify_import.py`,
`import_typeshed.py`) into r-typeshed. Decision from the project owner:
r-typeshed must not require Python. The stub toolchain is R (for scripts
that need a live R session: generation and auditing) plus ry itself (for
validation — the serde loader in `ry-typeshed` IS the normative schema, so
validating with the real parser is strictly stronger than a JSON Schema
check and cannot drift).

## ry side: new CLI subcommand

Add `ry typeshed validate <DIR>...` to `crates/ry-cli`:

- For each directory, discover stub files in both supported layouts
  (`<dir>/<pkg>.json` and `<dir>/<pkg>/<pkg>.json`) — reuse the discovery
  logic behind the existing runtime `load_stub_dir` (plan 03) rather than
  duplicating it; refactor to share if needed.
- Load every file through the real ry-typeshed parsing path (strict serde:
  `schema_version` supported, all fields well-formed). On top of parsing,
  enforce the repo conventions previously covered by validate.py:
  - `package` field matches the file stem / directory name.
  - No duplicate function names after alias expansion within a package.
  - Every `eval` mode / return slot / mode / length string is a known value
    (serde already guarantees this if the enums are strict — verify; where
    the JSON uses free-form strings like `mode`, add explicit validation
    against the known vocabulary in one place shared with the checker's
    JsonRType -> RType conversion).
  - Warn (not fail) on unsorted function keys.
- Output: one line per problem, `path: message`, summary line, exit 0/1.
  Also `--quiet` for CI (summary only).
- CLI wiring: this becomes a `typeshed` subcommand group; move the existing
  `ry explain typeshed` alias? NO — leave `explain typeshed` alone; just add
  the new `typeshed validate` subcommand. Update shell completions
  (clap_complete handles this automatically — verify).
- Tests: e2e tests covering a valid dir, a file with a bad mode string, a
  package-name mismatch, and a duplicate-alias collision.
- Also extend `scripts/sync_typeshed.sh` to run `cargo run -p ry-cli --
  typeshed validate` against the vendor dir after syncing, as a guard.

Check `crates/ry-cli/Cargo.toml` for the actual binary/package name before
wiring (`-p ry-cli` is an assumption; verify).

## r-typeshed side: remove Python, rewire CI

1. Delete `scripts/validate.py`, `scripts/verify_import.py`,
   `scripts/import_typeshed.py`, `scripts/__pycache__/`, and any other
   Python artifacts. (The import-verification job is complete — the data
   was verified lossless at import time; its provenance is recorded in
   CHANGELOG.md. Add one sentence there noting verification happened and
   that the verify script was removed with plan 08.)
2. Keep `scripts/gen_typeshed.R` and `scripts/audit_typeshed.R` unchanged.
3. If plan 04 added or extended any Python validation for the new schema
   fields (`schema_effect`, `higher_order`, `globals`), that coverage must
   exist in `ry typeshed validate` before deleting the Python.
4. Delete `schema/typeshed.schema.json` (a second machine-readable schema
   would drift; the ry loader is normative). `schema/SCHEMA.md` stays and
   becomes the single documentation source — update it to say validation is
   done by `ry typeshed validate` and that the serde definitions in ry's
   `crates/ry-typeshed/src/lib.rs` are normative.
5. Rewrite `.github/workflows/ci.yml`: install ry (download the latest
   GitHub release binary for linux x86_64 from
   https://github.com/sims1253/ry/releases — BUT the release binaries
   predate the `typeshed validate` subcommand, so until the next ry release
   the workflow must build from source instead: checkout sims1253/ry at
   `dev`? No — pin: add the build-from-source path now (actions/checkout of
   sims1253/ry + dtolnay/rust-toolchain + Swatinem/rust-cache + cargo build
   -p <cli package> --release), with a clearly marked TODO to switch to
   release-binary download after the next ry release), then run
   `ry typeshed validate stubs/` (adjust to however the command discovers
   the per-package subdirectories — pass `stubs/` once if it recurses, or
   glob the subdirs).
6. Update README.md: contribution workflow becomes gen_typeshed.R draft ->
   hand-curate -> audit_typeshed.R -> `ry typeshed validate` -> PR. Remove
   all Python references.

## Rules

- No mutating git commands in either repo; leave everything uncommitted.
- The ry-side subcommand must reuse the existing loader code paths — no
  parallel validation implementation that can diverge from what `ry check`
  actually accepts.
- Behavior of `ry check` unchanged: full test suite, corpus, vendor_snapshot
  green with no snapshot updates.
- No emojis.

## Acceptance criteria

- `rg -l "python|\.py" /home/m0hawk/Documents/r-typeshed --iglob '!*.json'`
  returns nothing (no Python references left in scripts/CI/README).
- `find /home/m0hawk/Documents/r-typeshed -name '*.py'` returns nothing.
- `cargo run -p <cli> -- typeshed validate /home/m0hawk/Documents/r-typeshed/stubs`
  exits 0 on the real data; deliberately corrupting a mode string in a temp
  copy makes it exit 1 naming the file (cover in e2e tests instead of
  manual corruption of the real repo).
- ry workspace: build/test/clippy/fmt all clean.
- CI workflow YAML is valid and self-consistent (build-from-source path).
