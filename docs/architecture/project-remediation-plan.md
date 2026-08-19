# Project Remediation Plan

## Status

Proposed plan based on the current-state architecture, implementation, UI/UX,
and testing review performed on 2026-08-12.

The checker core is capable and has strong semantic fixtures, but the project is
not release-ready. The immediate risks are at system boundaries: clean checkout,
incremental LSP state, editor configuration, extension packaging, and release
artifact verification.

This plan deliberately optimizes for a small number of meaningful end-to-end
journeys. It does not target a test-count or line-coverage percentage.

## Outcomes

When this plan is complete:

1. A clean checkout builds and passes every required gate without an untracked
   sibling repository.
2. The CLI and LSP use one workspace-analysis implementation for diagnostics
   and interactive features.
3. A live LSP session converges to the same result as a fresh session after
   open, edit, close, reopen, configuration, and workspace-folder changes.
4. VS Code settings and binary changes take effect without misleading status or
   requiring an undocumented reload.
5. Zed and VS Code release artifacts are tested as installed artifacts, and
   checksum failures block execution.
6. The required PR suite is centered on five end-to-end journeys, supported by
   semantic corpus, oracle, property, fuzz, and performance tests where those
   cover genuinely different risks.

## Working principles

- Restore release truth before adding analyzer features.
- Add a deterministic failing journey before fixing a discovered integration
  defect.
- Prefer parity assertions between real user surfaces over duplicate assertions
  inside each layer.
- Keep semantic corpus and oracle coverage; they are not interchangeable with
  editor or protocol E2E coverage.
- Do not preserve compatibility state indefinitely. Each new analysis seam must
  identify the old path it replaces and deletes.
- Tests must wait on observable state or protocol events, not arbitrary sleeps.
- Every subprocess and protocol wait must have a wall-clock timeout and useful
  failure output.

## Phase 0: Restore a truthful green baseline

No feature work should merge while this phase is incomplete.

### R0.1 Make clean checkouts self-contained

Current issue at review time (2026-08-12): the workspace pointed at
`../ry-diagnostics`, which is not part of this repository.

**Since resolved.** The `ry-diagnostics` dependency was removed outright
(a06df8b) rather than republished or vendored; no external path dependency
remains, and a tracked-only `git archive HEAD` extraction passes
`cargo check --workspace --all-targets --locked`. The clean-archive CI gate
below remains proposed work; the scheduled `clean-checkout` job in
`ecosystem.yml` approximates it from a fresh clone.

Work:

- Choose one reproducible dependency form:
  - preferred: publish and pin an exact compatible `ry-diagnostics` version; or
  - vendor it as a workspace crate if coordinated publication is not desired;
  - use a pinned Git revision only as a temporary bridge.
- Remove reliance on a sibling checkout from `Cargo.toml` and `Cargo.lock`.
- Add a clean-archive gate that copies or archives tracked files into a temporary
  directory and runs `cargo metadata` plus `cargo check --workspace --all-targets`.
- Run that gate in the primary CI workflow, not only in an ecosystem job.

Acceptance:

```sh
git archive HEAD | tar -x -C <temporary-directory>
cargo metadata --manifest-path <temporary-directory>/Cargo.toml --no-deps
cargo check --manifest-path <temporary-directory>/Cargo.toml --workspace --all-targets
```

### R0.2 Fix all required local gates

Work:

- Fix the mixed Mocha/`bun:test` compilation model so `bun run test` reaches the
  VS Code Extension Host.
- Consolidate the overlapping `test-code.yml` and `build-vscode.yml` workflows
  into one required workflow with named format, lint, type-check, unit, package,
  and installed-E2E stages.
- Keep the direct Bun unit command separate from the Mocha Extension Host suite,
  or standardize both on one runner; do not compile Bun-only tests as Mocha tests.

Acceptance:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

cd editors/code
bun run fmt-check
bun run lint
bun run tsc
bun test src/test/binary.test.ts
xvfb-run -a bun run test
```

### R0.3 Turn the LSP convergence failure into a deterministic regression

Current issue at review time (2026-08-12): closing and reopening a document
could publish no diagnostics while a fresh server published `RY090` and
`RY091`.

**Since resolved.** The failure was a race in the w10 test harness (the
close's clearing publication matched as the reopened document's
publication), not a server defect; it is fixed and regression-gated (#81).

Work:

- Extract the committed property-test seed into a named deterministic test.
- Determine whether the defect is stale project state or the harness observing
  the wrong publication.
- Assert document version, effective source, project membership, publication
  generation, and final normalized diagnostics at each step.
- Fix the lifecycle defect; keep the deterministic case in the fast PR suite.
- Retain the broader property test after the deterministic failure is green.

Acceptance:

- The named close/reopen journey passes repeatedly without retries.
- The property suite reports live-session equality with a fresh server.
- `cargo test --workspace` is green.

### R0.4 Enforce real Zed checksum verification

Work:

- Require the matching `.sha256` asset for every downloaded binary.
- Download and parse the sidecar, validate its filename/hash format, and compare
  it with the extracted executable.
- Delete the download and refuse to return a server command on missing,
  malformed, or mismatched checksums.
- Test the production verification path through an injectable release/download
  boundary rather than testing hash strings independently.

Acceptance:

- Correct sidecar: binary is accepted.
- Missing, malformed, and mismatched sidecars: binary is rejected and removed.
- The extension manifest's checksum claim matches actual behavior.

## Phase 1: Establish the five end-to-end journeys

These journeys become the product contract. New isolated tests should be added
only when they cover a lower-level invariant that would be awkward or opaque in
an E2E journey.

### E1. CLI package journey

Harness: real `ry` subprocess using `ry-testkit::FixtureProject` and the
`complete-package` fixture.

Boundaries:

```text
filesystem -> config discovery -> DESCRIPTION/NAMESPACE/data/typeshed
           -> parser/checker -> formatter -> stdout/stderr/exit status
```

Assertions:

- exact normalized diagnostics and fixes;
- package imports and user typeshed behavior;
- excluded and fixture files;
- severity and confidence filtering;
- baseline write, reuse, and invalidation;
- JSON structure and exit status;
- explicit missing targets fail instead of silently checking nothing.

Suggested location: `crates/ry-cli/tests/e2e.rs`.

### E2. Live editor-session journey

Harness: a real `ry server` subprocess over stdio with timeout-capable JSON-RPC.

Sequence:

1. Initialize a workspace.
2. Open a Unicode document and assert exact diagnostic code, UTF-16 range, and
   quick fix.
3. Apply an incremental edit and assert the diagnostic clears.
4. Close and reopen the file and assert disk/open-buffer precedence.
5. Change configuration and baseline state and assert republished diagnostics.
6. Shutdown cleanly and verify the child exits.

This journey owns the deterministic close/reopen regression from R0.3.

Suggested location: `crates/ry-lsp/tests/e2e.rs`.

### E3. Multi-root isolation journey

Harness: the same real LSP subprocess harness as E2.

Sequence and assertions:

- Initialize two roots with colliding function names and custom stubs.
- Give the roots different rule settings.
- Assert each document uses only its owning root's config, metadata, baseline,
  and stubs.
- Add and remove a workspace folder.
- Edit one root and assert the other root remains unchanged.
- Compare each final root with an independent real CLI invocation.

This journey should consolidate the highest-value cases currently spread across
the large P36 contract tests.

### E4. Installed VSIX journey

Harness: build the VSIX, install it into a fresh VS Code test profile, and use
the packaged extension with its actual bundled binary. Do not run only the
source-tree extension output.

Sequence and assertions:

- Open an R fixture and assert exact expected diagnostics.
- Apply a suppression quick fix and assert the document and diagnostics update.
- Change `ry.lint.ignore` or `ry.minConfidence` and assert live republishing.
- Change `ry.path`, invoke Restart Server, and verify both reported and observed
  server identity change.
- Invoke Explain Rule and assert that a populated Markdown document opens.
- Exercise an untrusted workspace containing a decoy `ry.path` and assert the
  bundled binary remains selected.
- Exercise missing/unexecutable binary behavior and assert an actionable visible
  error with an Open Logs or Configure action.

Suggested location: `editors/code/src/test/e2e.test.ts`, replacing the two
duplicate activation smoke tests.

### E5. Release artifact contract

Harness: artifact inspection in release CI before any publish job.

Assertions:

- cargo-dist archives contain the expected executable layout;
- the native host executable runs `ry version` and a small `ry check`;
- every downloadable archive has a valid checksum sidecar;
- the VSIX contains the correct manifest and target-specific bundled binary;
- the installed VSIX can start that bundled binary;
- Zed rejects missing, malformed, and mismatched checksum fixtures.

Publishing must depend on this contract job.

## Phase 2: Complete the single analysis boundary

### A2.1 Define one production workspace interface

Create a deep interface, provisionally `WorkspaceAnalysis`, that owns:

- workspace roots and stable file identities;
- disk content and open-buffer overlays;
- document versions and revisions;
- effective config, baselines, workspace metadata, and typesheds per root;
- parsing and project semantic state;
- diagnostics, hover, completion, signature help, navigation, symbols,
  inlay hints, and fixes;
- cancellation and stale-result rejection.

The API should accept changes and answer immutable-revision queries. Protocol
types must remain in the LSP adapter rather than leaking into analysis.

### A2.2 Migrate diagnostics first

Work:

- Move `ProjectCache` ownership behind `WorkspaceAnalysis`.
- Replace the duplicate CLI/LSP sequences of `Project::set_*` calls with one
  environment installation path.
- Make environment updates equality-aware so unchanged metadata does not mark
  every file dirty.
- Preserve CLI/LSP diagnostic parity using E1-E3.

Acceptance:

- CLI and LSP both call the same diagnostics query.
- A one-file edit does not re-emit unrelated files when configuration and
  dependencies are unchanged.
- Instrumented recomputation counts are bounded in a medium multi-file fixture.

### A2.3 Migrate interactive features

Work:

- Route hover, completion, definition, references, and signature help
  through the same revision used for diagnostics.
- Resolve symbol identity by binding and scope, not spelling alone.

Later revision: the cross-file half of this section was descoped, not
deferred. The P38 feature-differential file was deleted with B2–B5 accepted
as behaviour, `textDocument/rename` was removed outright (spelling-based
rename is unsafe under R's NSE and dynamic binding), and the interactive
handlers keep open-document scope (see
`docs/architecture/p38-progress.md`). The binding-resolution and
shared-revision requirements still apply to the open-document features that
remain.

Acceptance:

- Open-document hover, completion, definition, references, and signature
  help resolve bindings and scopes correctly.
- Diagnostics and interactive queries cannot observe different source
  revisions.
- Cross-file queries and `textDocument/rename` remain descoped (see the
  revision note above).

### A2.4 Remove compatibility state

After each query family migrates, delete the replaced state and fallback path.
The end state should not retain parallel `workspace_contexts`,
`folder_contexts`, legacy project caches, and single-file interactive checkers.

Split the LSP backend into focused modules around:

- document synchronization;
- workspace/configuration lifecycle;
- analysis adapter;
- diagnostic publication;
- protocol feature handlers.

## Phase 3: Make configuration and editor lifecycle truthful

### C3.1 Atomic configuration updates

Work:

- Distinguish missing config from malformed/unreadable config.
- Build a complete replacement folder context off-lock.
- On success, atomically replace config, filter, confidence, exclusions,
  baseline, stubs, and workspace metadata.
- On failure, retain the last valid context and send a visible actionable
  message.
- Add explicit VS Code configuration synchronization or notifications.

Acceptance:

- Live setting changes are reflected without a reload.
- A half-written `ry.toml` does not revert analysis to defaults.
- CLI and editor apply the same precedence rules.

### C3.2 Correct binary restart semantics

Work:

- Re-read current settings and re-resolve/probe the binary on every restart.
- Update the stored settings and `ResolvedBinary` only after successful
  validation.
- Keep the old healthy server running if the replacement binary is invalid.
- Ensure status and debug information report the binary actually running.

### C3.3 Remove or implement inert settings

For each exposed setting, either complete the behavior and cover it in E4 or
remove it from the schema and documentation:

- `ry.checkTestFixtures`;
- `ry.addExecutableToTerminalPath`;
- rule selection/extension settings;
- custom configuration path behavior.

## Phase 4: Tighten product UX and output contracts

### U4.1 Fix editor commands and failure feedback

- Correct Explain Rule command syntax and response model.
- Use argument-array subprocess APIs rather than shell command strings.
- Provide actionable errors for missing binaries, malformed configuration,
  incomplete indexing, and server startup failure.
- Surface discovery-limit warnings through a visible message with Show Logs,
  rather than hidden log output only.

### U4.2 Correct CLI edge behavior

- Fail on explicitly missing or unreadable input paths.
- Define and implement the exact contract for `-q` and `-qq`; update help text
  if complete silence is not intended.
- Emit terminal-clear control sequences in watch mode only when attached to an
  interactive terminal.
- Escape GitHub workflow-command fields and JUnit XML attributes correctly.
- Add these cases to E1 with hostile filenames and redirected output.

### U4.3 Restore documentation truth

- Generate or verify the README rule table from the registry so new codes do
  not disappear from documentation.
- Update editor limitations after unopened-file indexing and interactive
  migrations are complete.
- Do not record a plan as accepted while its required default gate is red.

## Phase 5: Consolidate and maintain the suite

### T5.1 Make `ry-testkit` the parity harness

Work:

- Add timeout-capable real subprocess transport with captured stdout/stderr.
- Centralize diagnostic, position, severity, and fix normalization.
- Express one fixture once and compare checker, CLI, in-memory LSP where useful,
  and real LSP observations.
- Remove duplicated normalization code from protocol and P36 contract tests.

### T5.2 Prune only after replacement coverage exists

Candidates:

- replace the duplicate VS Code activation smoke with E4;
- remove no-op corpus summary assertions that rerun the same fixture load;
- consolidate repeated single-diagnostic adapter tests into one parity contract;
- fold overlapping multi-root contract cases into E3.

Do not prune:

- exact semantic corpus fixtures;
- adjacent negative-control fixtures required by the false-positive bar;
- the R oracle;
- CLI/LSP differential behavior;
- UTF-16 protocol coverage;
- property invariants, fuzzing, or performance gates that cover distinct risks.

### T5.3 Define CI lanes by risk

Fast required PR lane:

- clean-checkout build;
- format, lint, Clippy, type-check;
- E1 CLI package journey;
- E2 live editor-session journey;
- E3 multi-root isolation journey;
- E4 installed VSIX journey;
- semantic corpus and core invariants.

Release lane:

- E5 artifact contract;
- clean installation and version/check smoke on produced native artifacts;
- checksum and publisher identity verification.

Scheduled or dedicated lane:

- R oracle matrix;
- ecosystem corpus and drift reconciliation;
- fuzzing;
- extended property seeds;
- release-mode performance and scaling budgets.

## Delivery order and merge boundaries

1. **Baseline PR:** self-contained dependency, Clippy fix, VS Code test-runner
   fix, deterministic LSP regression extraction.
2. **Lifecycle PR:** fix LSP close/reopen convergence and add E2.
3. **Artifact-security PR:** real Zed checksum verification and E5 foundation.
4. **Harness PR:** timeout-capable `ry-testkit` transport and shared observation
   model.
5. **CLI E2E PR:** add E1 and correct missing-target/output edge behavior.
6. **Analysis diagnostics PR:** move diagnostics behind `WorkspaceAnalysis`, add
   equality-aware environment updates, and prove incremental locality.
7. **Analysis features PRs:** migrate navigation, completion/hover, and signature
   help in small query-family slices; delete replaced compatibility code in each.
8. **Editor lifecycle PR:** atomic settings, binary re-resolution, actionable
   errors, and E4 installed-VSIX coverage.
9. **Multi-root PR:** complete E3 and remove duplicated contract harnesses.
10. **Consolidation PR:** merge VS workflows, prune replaced smoke tests, update
    documentation and acceptance records.

Each PR must leave all previously completed journeys green. Large architectural
changes should be sliced by end-to-end behavior, not by creating unused types or
interfaces first.

## Completion gate

This plan is complete only when all of the following are true:

- A tracked clean archive builds without sibling repositories.
- All documented required commands pass locally and in CI.
- No known failing test is described as an accepted green baseline.
- E1-E5 run in their assigned lanes and test real user-facing artifacts or
  processes.
- CLI and LSP diagnostic parity holds for package, Unicode, filtering,
  baseline, and multi-root cases.
- Live and fresh LSP sessions converge after every supported lifecycle change.
- VS Code settings, restart, Explain Rule, and quick fixes work through an
  installed VSIX.
- Zed refuses an artifact whose checksum cannot be authenticated.
- The old duplicate analysis and compatibility paths have been removed.
- Test growth is justified by a new risk or invariant, not a coverage-number
  target.
