# Plan 03: Vendored typeshed snapshot + runtime stub loading in ry

## Status: blocked on plan 02 (r-typeshed repo bootstrap). Implement after 02.
## Repo: ry (this repo).

## Context

After plan 02, stub data lives in the standalone repo
`github.com/sims1253/r-typeshed` (local checkout:
`/home/m0hawk/Documents/r-typeshed`) with per-package files
`stubs/<pkg>/<pkg>.json`, a `schema_version`/`package` header, and bayes.json
split into five per-package files. ry must now:

1. Vendor a snapshot of that data instead of owning it in
   `crates/ry-typeshed/data/`.
2. Allow users to supply extra/override stub directories at runtime via
   `ry.toml` (no recompile needed to iterate on stubs) — the equivalent of
   ty's `--custom-typeshed-dir`.

## Design

### Vendoring

- New directory `crates/ry-typeshed/vendor/` containing a snapshot of the
  r-typeshed repo's `stubs/` tree plus a `SOURCE` file recording the upstream
  repo URL and commit hash it was synced from.
- `crates/ry-typeshed/data/` is DELETED; all `include_str!` paths point into
  `vendor/`.
- Because bayes.json is now five real per-package files, delete the
  prefix-stripping machinery in `ry-typeshed/src/lib.rs` (`PackageSpec.prefix`,
  the `parse_package` prefix argument, and the multi-package handling). Each
  `PackageSpec` maps 1:1 to one vendored file. Keep `known_packages()`,
  `is_known_package()`, `load_base_cached()`, `load_package()` signatures
  unchanged.
- Teach the serde structs about the new optional header fields
  (`schema_version`, `package`): add them as optional fields, and when
  present assert `schema_version == "1"` at load time (return a
  `TypeshedError::UnsupportedSchema` otherwise).
- New script `scripts/sync_typeshed.sh`: copies `stubs/` from a local
  r-typeshed checkout (path argument, default `../r-typeshed`) into
  `crates/ry-typeshed/vendor/`, writes `SOURCE` with `git -C <checkout>
  rev-parse HEAD`. (Reading the other repo's HEAD via `git rev-parse` is
  permitted; do not run mutating git commands.)

### Runtime stub loading

- New API in ry-typeshed:
  ```rust
  /// Load stub files from a user-supplied directory. Accepts both layouts:
  /// flat (`<dir>/<pkg>.json`) and nested (`<dir>/<pkg>/<pkg>.json`).
  /// Returns package name -> Typeshed. Errors carry the offending path.
  pub fn load_stub_dir(dir: &Path) -> Result<BTreeMap<String, Typeshed>, TypeshedError>
  ```
  The `package` header field is the package name; fall back to the file stem
  when absent. A user stub for a package ry already embeds REPLACES the
  embedded one wholesale (document this; per-function merging is out of
  scope).
- Resolution layering (checker side): user stubs > embedded vendored stubs.
  Find where the checker resolves signatures (`Checker::resolve_typeshed_sig`
  in ry-checker and `load_package` call sites) and thread an optional
  `Arc<BTreeMap<String, Typeshed>>` of user stubs through `Checker` and
  `Project` (a `set_user_stubs(...)` setter, mirroring the existing
  `set_loaded`/`set_external_bindings` pattern in
  `crates/ry-checker/src/project.rs`). `is_known_package` checks must also
  consult user stubs (a package with user stubs counts as known). Also cover
  the base typeshed: a user dir may contain `base.json` which then overrides
  the embedded base typeshed for that run.
- Config: `crates/ry-cli/src/config.rs` gains a `typeshed` key in `ry.toml`:
  ```toml
  typeshed = ["path/to/stubs", "another/dir"]
  ```
  Paths are resolved relative to the ry.toml location. Also add a
  `--typeshed <DIR>` CLI flag (repeatable) that appends to the config list.
  Later directories override earlier ones; CLI flags override config.
- LSP: `crates/ry-lsp` builds Projects too — wire the same user-stub loading
  from the workspace's ry.toml so editor diagnostics match CLI output. Follow
  how the LSP currently discovers ry.toml settings; if it does not read
  ry.toml at all today, add loading only if it is cheap to do so, otherwise
  leave a TODO and note it in the final summary.
- Diagnostics for bad stub files: a malformed user stub file must not abort
  the run; emit a warning to stderr (CLI) / log (LSP) naming the file and
  continue without it.
- `ry explain typeshed` (see `run_explain_typeshed` in
  `crates/ry-cli/src/main.rs`) should list: vendored snapshot source/commit
  (from a compile-time embedded `SOURCE` string), embedded packages, and any
  active user stub dirs with the packages they provide.

## Tasks (suggested order)

1. Sync script + vendor directory + rewire `include_str!` + delete
   prefix-stripping; keep all existing ry-typeshed tests passing (update the
   bayes prefix tests to the new per-package files).
2. Schema header fields + validation.
3. `load_stub_dir` + unit tests (tempdir fixtures: flat layout, nested
   layout, override-embedded, malformed file, missing package field).
4. Thread user stubs through Checker/Project + checker-level test: a user
   stub dir defining a fake package `foo` with function `bar` returning
   integer; `library(foo); x <- bar() + 1L` produces no diagnostics, and
   without the stub dir it produces the unknown-binding behavior it does
   today.
5. ry.toml + CLI flag + e2e test in `crates/ry-cli/tests/config_e2e.rs`
   style.
6. LSP wiring (or documented TODO).
7. Update README.md (new "Custom typesheds" section) and CHANGELOG.md
   (Unreleased section).

## Rules

- Do NOT run mutating git commands; read-only `git rev-parse` in the sync
  script is fine. Leave changes uncommitted.
- Public behavior without any `typeshed` config must be byte-identical to
  today (snapshot tests in `crates/ry-checker/tests/vendor_snapshot.rs` must
  pass unchanged).
- Keep `ry-typeshed`'s public API additive.
- No emojis. Match existing comment style (dense doc comments explaining WHY).

## Acceptance criteria

- `cargo test --workspace` passes; vendor_snapshot unchanged.
- New tests cover: flat/nested stub dirs, override of embedded package,
  override of base, malformed stub file warning path, ry.toml + CLI flag
  layering.
- `crates/ry-typeshed/data/` no longer exists; `vendor/SOURCE` records the
  r-typeshed commit.
- `rg "brms\." crates/ry-typeshed/src` shows no remaining prefix-stripping
  logic.
- `cargo clippy --workspace --all-targets`: no new warnings. `cargo fmt --check` passes.
