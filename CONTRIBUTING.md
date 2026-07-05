# Contributing

Start with `ARCHITECTURE.md` for the lay of the land.

## Build and test gate

Every change must pass, in this order:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

If R is installed, also run the oracle (CI always does):

```sh
cargo test -p ry-checker --test oracle -- --ignored
```

The oracle uses a parallel R driver (`scripts/oracle_driver.R`, purrr +
mirai) when those packages are available and falls back to one
`Rscript` process per fixture otherwise.

## Fixture conventions

- `crates/ry-checker/testdata/ok_*.R` -- must produce zero diagnostics.
  First line `# no-diag`.
- Other corpus fixtures declare the expected rule; see
  `crates/ry-checker/tests/corpus.rs` for the expectation syntax.
- `testdata/oracle/*.R` -- first line is `# oracle: must-pass`,
  `# oracle: must-flag`, or `# oracle: known-gap <one-line reason>`.
  R executes these files; keep them side-effect-free.
- Vendored packages live in `testdata/vendor/<pkg>/` with their
  LICENSE. Snapshots are updated with
  `INSTA_UPDATE=always cargo test -p ry-checker --test vendor_snapshot`
  and every diagnostic in a snapshot must be triaged in the comment
  block of the corresponding test.

## The false-positive bar

ry prefers silence over noise. A new rule (or a widened existing rule)
merges only if:

1. it has corpus fixtures for both the bug and the adjacent idiom that
   must stay quiet,
2. the oracle can arbitrate it where R's runtime behavior is the ground
   truth, and
3. it produces zero unexplained findings on the vendored CRAN code.

When inference is uncertain, return `unknown` and say nothing.

## Typeshed changes

Never add a function name you have not verified against R.
`scripts/audit_typeshed.R` checks every declared name with `exists()`
(base) or against the package namespace (package files) and runs in CI.
`scripts/gen_typeshed.R <pkg>` drafts a stub file from a package's
exports for hand-refinement.

## Style

- Match the existing comment style: comments explain R semantics and
  design constraints, not what the next line does.
- Conventional-commit subjects, as in the log
  (`fix(scope): ...`, `feat(area): ...`, `test: ...`).
- No emojis anywhere in code, comments, docs, or commit messages.
- `README.md` is generated from `README.Rmd`
  (`Rscript -e 'rmarkdown::render("README.Rmd")'` with the release
  binary on PATH) -- edit the `.Rmd`, commit both.
