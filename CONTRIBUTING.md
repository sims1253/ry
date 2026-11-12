# Contributing

## Build and test gate

Every change must pass, in this order:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

The default oracle gate checks semantic-claim completeness and, when R is
installed, executes every registered claim fixture:

```sh
cargo test -p ry-checker --test oracle
```

Run the complete checker-vs-R matrix as well when R is installed (CI always
does):

```sh
cargo test -p ry-checker --test oracle -- --include-ignored
cargo test -p ry-cli --test e2e facts_oracle -- --include-ignored
```

Each oracle fixture runs in a fresh `Rscript --vanilla` process. Its exit
status determines whether R errored; fixtures cannot leak bindings, attached
packages, or daemon state into later fixtures.

The [instruction-count corpus guide](docs/corpus/instructions.md) explains
the fixed performance sample, local measurements, and warn-only CI deltas.
Run `python3 ecosystem/test-instructions.py` after changing that harness.
The [performance tracking guide](docs/performance.md) covers core timings,
extension activation, package sizes, and the historical dashboard.

## Fixture conventions

- `crates/ry-checker/testdata/ok_*.R` -- must produce zero diagnostics.
  First line `# no-diag`.
- Other corpus fixtures declare the expected rule; see
  `crates/ry-checker/tests/corpus.rs` for the expectation syntax.
- `testdata/oracle/*.R` -- first line is `# oracle: must-pass`,
  `# oracle: must-flag`, `# oracle: must-warn RYxxx`,
  `# oracle: must-flag-only RYxxx`, or
  `# oracle: known-gap <one-line reason>`. A `must-warn` fixture uses
  R-side assertions to establish the behavior and requires that ry emit
  the named warning. A `must-flag-only RYxxx` fixture requires that ry
  emit that code and no other diagnostic, pinning the whole-file
  suppression of semantic rules on files R cannot parse. R executes
  these files; keep them side-effect-free.
- Every active entry in `ry_checker::rules::RULES` must have at least one
  fixture containing `# oracle-claim: RYxxx`. The R code must demonstrate the
  premise stated by that diagnostic with an error, warning, `quote()` shape,
  or explicit assertion. A `known-gap` fixture cannot count as a claim. Adding
  a rule without a registered claim fixture fails the default oracle test.
- `crates/ry-checker/tests/oracle-known-gaps.txt` holds the number of
  `known-gap` fixtures, and the default oracle test fails when it differs.
  A pull request that raises the number fails the Known-gap budget check
  until a maintainer adds the `known-gap-increase` label. Removing a gap
  lowers the number; write the new count to the file.
- Every rule also needs a worked example in
  `crates/ry-checker/src/rule_examples/RYxxx.txt`, which `ry explain rule`
  prints. The file starts with a `fix:` line, then holds files that each
  start with a `--- <path>` line: `flagged.<ext>`, `fixed.<ext>`, and any
  support files such as `ry.toml`. The e2e test `rule_examples` checks that
  the flagged file reports only that rule and that the fixed file reports
  nothing.
- An oracle fixture can also check `ry dump-facts` against R. Add one or
  more `# oracle-facts: name ...` lines naming top-level bindings. The file
  must run without error, so tag it `# oracle: must-pass`. The facts oracle
  exports the file with `ry dump-facts`, then runs it in `Rscript --vanilla`
  with a read-only probe appended. It fails when R contradicts a concrete
  claim: `mode` against `typeof()` (a union allows its members' modes), a
  known, `nonempty`, or `at_least` `length` against `length()`, and a known
  `class` against `oldClass()`. A declared binding that ry exports with no
  mode, length, or class claim also fails. The probe sees only the global
  environment, so to check a function's result, assign it at top level.
  Use base R only. Do not commit a fixture for a fact that ry gets wrong;
  add it with the fix.
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

## Evidence for semantic changes

For semantic, performance, and concurrency changes, the PR description should:

- name the mechanism it changes, a witness that should change, an existing
  true positive that must remain, and an adjacent idiom that must stay quiet;
- back R-semantics claims with `Rscript --vanilla` output, ideally as oracle
  fixtures, and speed claims with a fixed workload;
- list the exact commands run, their results, and any check that did not run
  (a missing tool or unrun comparison is not a pass);
- call out changes to oracle premises, `known-gap` tags, triage labels,
  benchmark workloads, and baselines for explicit review.

Aggregate corpus totals and updated expected values are not evidence that
individual findings were preserved. For a rejected approach, record the
invalid assumption and the smallest counterexample in the issue.

## Typeshed changes

Never add a function name you have not verified against R.
Stubs live in the standalone
[r-typeshed](https://github.com/sims1253/r-typeshed) repository. Its CI
runs the auditing and stub generation.
`scripts/sync_typeshed.sh <checkout>` vendors a snapshot into
`crates/ry-typeshed/vendor` and validates it with `ry typeshed validate`.
Change stubs upstream, not in `vendor/`: `scripts/test_sync_typeshed.py`
checks the vendored tree against the hash recorded in `vendor/SOURCE`.
`crates/ry-typeshed/build.rs` embeds every vendored stub. Signature lookup
follows package-name order.

## Editor extensions

The VS Code / Positron extension lives in `editors/code/`. It uses
[bun](https://bun.sh) for package management and esbuild for bundling.

### Running from source

```bash
cd editors/code
bun install
bun run compile
```

Press `F5` in VS Code to launch an Extension Development Host with ry
loaded. The `ry` binary must be on your `PATH`, or placed in
`editors/code/bundled/bin/`.

To run the Linux extension tests, follow the
[end-to-end testing guide](docs/testing.md#run-the-suite).

### The `--no-dependencies` rule

Package VSIXs with `--no-dependencies`:

```bash
bunx @vscode/vsce package --no-dependencies
```

This is required because `vsce`'s dependency walker cannot read
`bun.lock`, so a non-bundled extension mis-resolves `node_modules`.
Bundling with esbuild avoids this.

### Zed extension

The Zed extension lives in `editors/zed/`. It has its own `Cargo.lock`
and is excluded from the root workspace because `zed_extension_api` is
not held to ry's MSRV.

```bash
cargo build --manifest-path editors/zed/Cargo.toml
cargo test --manifest-path editors/zed/Cargo.toml
```

## Style

- Match the existing comment style: comments explain R semantics and
  design constraints, not what the next line does.
- Conventional-commit subjects, as in the log
  (`fix(scope): ...`, `feat(area): ...`, `test: ...`).
- No emojis anywhere in code, comments, docs, or commit messages.
- Keep `README.md` focused on installation and getting started. Put detailed
  usage, configuration, and rule reference material in `docs/`. Update
  `docs/rules.md` when the rule registry changes. Keep examples
  self-contained or backed by fixtures/tests.
