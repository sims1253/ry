# Changelog

All notable changes to ry are documented in this file.

## [Unreleased]

### Added

- Add RY111 (`constant-argument-shadowing`): a call argument that passes
  the reserved-word literal `TRUE`/`FALSE` for a formal an enclosing
  function exposes under the identical name -- silently hardcoding
  instead of forwarding the caller's value. The founding fixture is
  haven @ f067fb2, `R/labelled.R:111`:
  `median.haven_labelled <- function(x, na.rm = TRUE, ...) { ...
  median(vec_data(x), na.rm = TRUE, ...) }` (runtime-verified in #361:
  `median(labelled(c(1:4, NA), c(a = 1)), na.rm = FALSE)` returns 2.5
  where base `median(c(1:4, NA), na.rm = FALSE)` returns NA -- the
  caller's explicit argument is silently ignored, whatever the
  default). Four gates keep the rule high-precision (#361):
  the tag must exactly name a formal of an enclosing function
  (innermost frame outward, so a capturing closure still orphans the
  outer caller's value while a closure with its own identically-named
  formal is judged by its own frame) AND a formal of the resolved
  callee (a collected user signature or a typeshed stub, the same
  resolution precedence RY090/RY091/RY092 use), the owning function
  must never read the formal anywhere in its body (the dead-formal
  gate), the value must be the literal itself, and unresolvable
  callees stay silent. The dead-formal gate is what the tidyverse
  corpus forced: a body that reads the formal anywhere -- an
  `if (p)` guard, a `f(p)` validation, a by-name forward at another
  call (`quiet = quiet`), a `missing(p)` test, a nested closure's
  captured read -- demonstrably handles the caller's value, so
  per-site constants there are chosen child semantics (dbplyr's
  `sql_render.*` methods forwarding `subquery` at their own wrapper
  while pinning child renders, stringr's `if (ignore_case)` early
  return before a fixed `regex(...)`, tibble's `quiet = quiet` at
  the user-facing call, dplyr's `if (recursive)` branch, rvest's
  `env_has(env, nm, inherit = inherit)` -- all silent). haven's
  founding fixture reads `na.rm` nowhere: the formal exists only in
  the signature, which is exactly the dead binding the constant then
  silently replaces. What else stays
  quiet: renaming-and-defaulting idioms (`remove_na` enclosing formal
  with an inner `na.rm = TRUE`), partial tags (`na.r = TRUE`, which R
  binds through partial matching but whose silence is the pinned
  precision trade), forwarding (`na.rm = na.rm`) and every computed
  expression, `T`/`F` (ordinary rebindable identifiers), `NA`,
  numeric and string constants (divergent defaults like an internally
  reformatted `sep` are an ordinary idiom in a way logical flags are
  not), `...`-absorbing and unknown callees (the literal's destination
  is unknowable, RY090's dots humility), and top-level code. Base
  `median`'s `na.rm`/`...` formals arrive as a ry-side
  `overlay/base.json` entry (`formals(median)` is `function (x,
  na.rm = FALSE, ...)`, R 4.6.1; the vendored stub declares only `x`),
  pinned by a drift test so an upstream widening forces a conscious
  merge; the entries stay untyped and non-required so no arity rule
  changes behavior. Corpus deltas (both manifests regenerated and
  strict-gated): tidyverse +1, the founding haven defect itself (the
  regenerated jsonlite report also gains one unaudited, deliberate
  identity: the package's own `stop` wrapper forcing `call. = FALSE`,
  explained in the ledger note); posit
  +20 -- 17 true positives (the dbplyr `sql_render.*_query` family,
  whose own `subquery` formal is never read even though the generic
  forwards the caller's flag into the method, and whose child renders
  pass `subquery = TRUE` to queries that may themselves be unions;
  reticulate's `r_to_py.POSIXt` (the method POSIXct/POSIXlt dispatch to) and `r_convert_dataframe_column`
  dropping their `convert` contract parameter; shiny's `observeEvent`
  pinning `autoDestroy = TRUE` while forwarding every sibling formal;
  torch's `nnf_rrelu_` forcing `training = TRUE` against its own
  `training = FALSE` default) and 3 false positives (gt's testthat
  helpers exposing testthat-compat `all = TRUE` signatures, sparklyr's
  `stream_read_socket` generic-family `columns` compat). Every other
  corpus site the ungated rule would have flagged is a deliberate
  idiom silenced by the dead-formal gate.

- Extend RY110 (`vacuous-all-guard`) across the same-file helper boundary:
  a single-formal function the checked file defines whose body is (or
  returns) the recognized guard chain -- hms's
  `is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))` --
  now registers as a guard-helper, and call sites that pass the guarded
  value onward arm its guard over the actual argument. Three application
  shapes qualify: a direct helper-call guard condition
  (`if (!is_numeric_or_na(v)) stop(...)`, including the positive
  `if (is_numeric_or_na(v))` then-branch and `stopifnot()`), and the
  args.R elementwise form (`valid <- map_lgl(args, is_numeric_or_na)`
  through base `lapply`/`sapply`/`vapply` or the purrr `map` verbs, read
  resolution-independently, followed by an `all(valid)` rejection). The
  registry builds per file from that file's own top-level definitions,
  so use-before-def order cannot hide a helper while the scan stays
  O(the file's own functions) for the `scaling_project_size` perf
  budget. The diagnostic still points at the helper's `all()` -- that
  definition is the defect, so each helper reports once -- while the
  emptiness premise and the recorded type come from the caller's
  binding. The downstream-demand gate is unchanged (a stub-declared
  parameter type in the applying function, same binding, same shadowing
  and rebinding discipline), which is what keeps the corpus silent:
  discarded helper results, multi-formal bodies, shadowed helpers or
  map callees, qualified helper calls, and extra call arguments all stay
  quiet, as does a helper applied from another file (the diagnostic
  could not point at the helper's span without attributing a foreign
  byte range to the consuming file). Two halves remain open: a guard
  validated in one function but demanded in another (a validator-summary
  hop, for the #351 flow-sensitivity cycle) still stays silent, and the
  `vctrs::vec_cast()` demand itself is now stubbed: a ry-side
  `overlay/vctrs.json` entry gives `vec_cast` an `x` carrying the
  Math-group numeric union (R: `vec_cast(character(), double())`
  errors), which arms the founding hms guard-helper shape. `vec_cast`
  is relationally polymorphic -- `x` need only be castable to `to`, so
  `vec_cast("foo", glue())` is legal -- and a plain numeric `x` type
  would false-positive RY092 on such calls; the stub therefore marks
  `x` with the new `demand_only` parameter flag, which exempts it from
  RY092's provable-incompatibility check while the RY110 demand gate
  still reads it. Upstream r-typeshed carries only the untyped
  `vec_cast_common`, so the entry lives outside the synced `vendor/`
  snapshot and is merged over it at load time -- the weekly `Typeshed
  bump` wholesale-replaces `vendor/` without touching it -- pinned by a
  test that fails loudly if upstream ever ships its own `vec_cast`;
  the sync provenance (`SOURCE`) is write-only (#479).

### Fixed

- Refresh the language server's on-disk R index when R sources change
  outside open buffers, and re-read a file from disk when it is closed.
  The server registered no watched globs for R sources and discarded
  their watched-file events, so editing, creating, or deleting an
  unopened `.R` file left cross-file diagnostics stale until some
  unrelated event (a config, `DESCRIPTION`/`NAMESPACE`, native-source, or
  serialized-data change) triggered a full reindex; and closing an
  edited-and-saved document fell back to the last indexed snapshot —
  typically the initialize-time content — instead of the saved bytes.
  R source globs now join the watcher registration, watched-file
  create/change/delete events refresh (or drop) the affected index entry
  through the same bounded decoder the background indexer parses with,
  and `did_close` re-reads the closed path from disk. Open buffers stay
  authoritative throughout: the per-file refresh never touches a live
  document, a concurrent `did_open` racing a refresh wins, and a stale
  in-flight background pass cannot overwrite fresher per-file entries
  (a retired initial pass is replaced so publications never strand
  behind `initial_index_pending`). The publish path still performs no
  disk I/O — freshness work happens in the watcher and close handlers —
  and admission runs through the shared per-file eligibility policy
  (excludes plus the size and depth caps, testthat runner
  classification, symlink and pruned-directory rules) so a rescan
  cannot disagree about membership (#486).
- Keep open documents the background discovery omits out of
  project-wide analysis too: a file over `index.max-file-bytes` or
  below a pruned `index.max-depth` used to enter the language server's
  shared project as soon as it was opened — the open-document
  eligibility gate enforced `exclude` patterns and folder enablement
  only — so the identical buffer resolved its definitions into every
  other open file while the closed index omitted it, contradicting the
  documented editor contract (#488). Eligibility now runs through one
  shared policy between the walker and buffer admission: the size gate
  measures the open buffer's text length (unsaved pasted content can
  cross the boundary without touching disk, so on-disk metadata must
  not stand in) and the depth gate counts the containing directory's
  components relative to the folder root, since walk depth is not a
  path property. Ineligible open documents stay out of binding and
  diagnostic state while keeping single-document editor features such
  as inlay hints; edits or configuration changes crossing a boundary
  eject or re-admit the buffer, and its stale diagnostics clear
  through the shared dropped-URI reconciliation. A buffer that grows
  past the size cap also shadows its own on-disk twin (which the
  index may still hold at the old, small size) so the path cannot keep
  contributing through the stale twin.
- Isolate nested R packages under one editor workspace folder into
  per-package analysis scopes instead of checking the whole folder
  through a single pooled project (#487): multiple packages sharing a
  workspace folder used to have their top-level functions and bindings,
  pooled `known_vars`, and `library()` attachments merged into one
  project, so a package-private binding defined in one package resolved
  in its sibling (hiding real RY010 findings) and a function name
  defined in both packages resolved to whichever definition the merge
  order preferred. Each folder now partitions its files by
  nearest-`DESCRIPTION` ancestor — the same
  `ry_workspace::group_by_package_root` boundary `ry check` partitions
  on, moved into the shared workspace crate so the two frontends cannot
  drift — and checks each package through its own project cache with
  its own workspace resolution context. Files in the same package still
  share definitions exactly as before, and plain multi-file scripts
  outside any package keep their visibility among each other as before;
  only the cross-package leak is closed — a loose script next to a
  package no longer resolves that package's private bindings, matching
  `ry check`. A package group whose files arrived
  after the last background index (for example a freshly created
  package) checks against an empty resolution context until the next
  index resolves it, rather than inheriting a sibling package's
  metadata.
- Advance the language server's per-package resolution context when a
  watched R-source refresh lands, instead of waiting for the next full
  background scan (#527): a created or externally edited file used to
  reach the disk index with a fresh parse but no resolution entries, so
  until some unrelated event triggered a rescan it checked against an
  empty context — a configured `globals` name fired a spurious RY010,
  its `library()` attachments stayed unbound, its package's
  `NAMESPACE`/`DESCRIPTION`-derived imports were missing, and an edit
  that moved a `load()` call kept the old span-keyed bindings — while
  its neighbors and a fresh server agreed on the correct result. Each
  landed refresh (and the close-time re-read in `did_close`) now
  re-resolves its owning package group through the same
  `resolve_workspace_context` pass the scan uses, over the current disk
  index scoped to the owning folder, installing group-keyed under the
  same generation guard every other index writer honors (one retry,
  then a full scan converges, on a lost race). Disk-only inputs mirror
  the scan exactly — open buffers keep shadowing at publish time — so
  the incremental install cannot disagree with a fresh scan over the
  same tree, existing files' entries recompute deterministically from
  unchanged inputs, and same-path concurrent refreshes stay ordered by
  the per-file generation protocol with no new interleave for a future
  per-path refresh epoch to untangle (#538).
- Republish tracked closed-file diagnostics after a watched refresh
  lands with zero documents open (#528): the project publish pass that
  recomputes unopened files' diagnostics only ran when an open document
  scheduled it, so fixing an indexed file on disk (or rescanning to new
  state) refreshed the parse without republishing — the client kept the
  pre-fix squiggles until some unrelated document opened, the exact
  trust failure the dropped-URI reconciliation had fixed for the
  open-document paths. Landed refreshes and landed rescans now schedule
  their paths through the existing debounce when no open document
  drives a pass (a no-op otherwise), and the publish itself only reads
  state and emits notifications, so it can never retrigger the
  scheduling — no loop. The still-indexed, still-eligible fixed file is
  recomputed and republished within the debounce window; made-ineligible
  paths keep clearing through the existing reconciliation.
- Clear stale diagnostics from every document the language server stops
  analyzing, not just the last-scheduled one (#489): when a folder was
  disabled or files excluded, `republish_all_open_documents` scheduled
  one debounced task per open document, but the debounce's single
  workspace-wide generation kept only the last task alive, so every
  URI but the last scheduled kept its previous squiggles indefinitely
  (which one survived followed HashMap iteration order), and closed
  disk files a rescan dropped from the index were never published
  again either. Every scheduled URI now joins one pending set that the
  surviving debounce task drains and publishes together, and the server
  tracks which URIs last received diagnostics so each publish pass —
  or a rescan with no open document to drive one — sends an empty
  publication for URIs that left the eligible set. Removing a
  workspace folder likewise clears the closed, previously-published
  disk files under the removed root, not just its open documents.
- Anchor the language server's config-relative paths at the directory of
  the `ry.toml` being used, matching `ry check`: a parent-discovered or
  explicitly configured `ry.toml` outside the workspace folder used to
  have its `exclude` patterns matched, `include-build-ignored` resolved,
  and baseline keys normalized against the folder root instead of the
  config's own directory, so an `exclude = ["pkgA/R/generated/**"]` in
  `/repo/ry.toml` never fired for an editor opened at `/repo/pkgA` and a
  CLI-generated baseline's `pkgA/R/…` keys never matched the editor's
  `R/…` keys, resurfacing baselined findings. Each folder context now
  records the config's origin directory and every config-relative
  resolution — exclude matching in eligibility and publication,
  `include-build-ignored` in background indexing, and baseline key
  normalization — anchors there, the same origin the CLI keeps from
  `Config::discover`; an in-folder `ry.toml` still anchors at the folder
  itself, so only inherited or external configs change behavior, and
  sibling workspace folders sharing a parent config each anchor it
  independently (#493).
- Stop the language server from keeping obsolete custom typeshed stubs
  after a config reload removes the last configured directory (#494):
  the reload treated an intentionally empty stub map (the `typeshed`
  setting cleared or deleted) the same as a failed one, so a warm
  session retained signatures a fresh server no longer loaded until
  restart. The stub loader now reports whether every configured
  directory failed, and only that genuine failure retains the previous
  stubs; clearing the list converges with a fresh server on the same
  config.
- Demote diagnostics from a package's `tests/`, `data-raw/`, `demo/`,
  `vignettes/`, and `inst/` trees one confidence tier in the language
  server too, through the same shared post-processing seam `ry check`
  uses, before the min-confidence threshold and before baseline
  subtraction. The server previously filtered on the checker's raw
  confidence, so equal thresholds selected different findings in the
  editor and on the command line: a medium-confidence RY010 in
  `tests/testthat/` was dropped by `ry check --min-confidence medium`
  but stayed visible at `ry.minConfidence: "medium"`. The demotion
  stage lives in `ry_checker`'s pipeline now and both frontends invoke
  it at the seam, keeping the nearest-`DESCRIPTION` root handling for
  nested packages and leaving severity untouched; a `tests/` directory
  with no package root above it is still not demoted (#492).
- Fail `ry check` when an explicitly requested input path does not exist,
  or a discovery root cannot be read, instead of silently succeeding with
  an empty or partial check. A missing path used to fall into the
  directory branch of discovery, whose failed `read_dir` was swallowed,
  so `ry check misspelled.R --output-format json` exited 0 with `[]` --
  a typo'd CI path looked like a clean check. Missing inputs are now
  each reported on stderr (`ry: <path>: no such file or directory`,
  matching `ry dump-types`) and the run aborts with exit code 1 before
  checking anything; `--exit-zero` still defuses it. An unreadable
  directory is carried as a discovery read error (same `ry: <path>:
  <error>` shape as parse read failures): it fails the exit code like a
  parse error while readable sibling inputs still get checked, and
  stdout keeps a well-formed empty report on every failure path. An
  existing-but-empty or fully excluded tree remains a successful run
  with the informational "no .R / .r files found" note (#485).
- Recognize a source-level `return(...)` in the shared block-divergence
  analysis (RY108 `seq-defaulted-forward` and RY110 `vacuous-all-guard`):
  the parser lowers the `return` keyword to an ordinary call, so the most
  idiomatic R reject-guard -- `if (!(G)) return(NULL)` guarding the code
  that follows -- is now judged by one shared view instead of the two
  rule-local copies #478/#480 had to add. A `base::return(...)`
  qualification is newly recognized by RY110 (RY108 already matched the
  qualified form); `return` inside a nested closure or a called helper
  still exits only that callee and never diverges the enclosing block.
  The journal's continuation facts deliberately keep the return-blind
  view: `if (is.null(x)) return(NULL)` still leaves `x` at its stale
  default binding so a following condition can fire RY001 on the
  zero-length shape, the pinned
  `null_return_guard_alone_does_not_prove_non_empty` behavior (#482).
- Flag a leading UTF-8 byte order mark as `RY000` instead of checking
  clean (#474): a BOM (`EF BB BF`) is valid UTF-8, but R's parser
  rejects the file with "unexpected input" at 1:1 in three of four
  execution contexts — `parse()`, `source()`, and `Rscript file.R`;
  only `parse(keep.source = TRUE)` strips it (verified against R
  4.6.1, including comment-only files, which R rejects unlike
  invalid bytes in comments). The shared read boundary behind the
  non-UTF-8 flag (#376) now also reports the BOM, so `ry check` and
  the LSP agree; a flagged file reports only its `RY000`. A U+FEFF
  character anywhere after the first byte is an ordinary character R
  accepts, and stays clean.
- RY105 (`constant-length-comparison`) and RY107
  (`any-all-scalar-comparison`) now fold a leading unary minus when
  extracting the comparison literal, so constant-outcome comparisons
  against negative bounds are reported instead of staying silent (#477):
  `any(x) > -1` is always TRUE (FALSE and TRUE both coerce past -1) and
  `length(sum(v)) > -1` is always TRUE for a length-1-by-construction
  operand, so those dead guards now warn, while `any(x) < -1` / `== -1`
  report as always FALSE. Value-preserving comparisons (`any(x) > 0`) and
  deliberate scalar assertions (`length(sum(v)) == 1`) stay quiet, and a
  minus over a non-literal (`-2^2` is `-(2^2)` in R) still does not fold.
  The RY105 message now reads "this length guard" instead of "this
  zero-length guard", since the admitted bounds go past zero. Unary `+`
  needs no folding of its own: the parser already lowers `+2` to the bare
  literal, so a plus-spelled bound behaves exactly like its bare spelling.
- Stop `ry check --write-baseline` from loading and subtracting the
  `baseline` configured in ry.toml before overwriting the file. The
  clap-level conflict only covers the `--baseline` spelling, so a
  configured baseline arrived through config merging anyway: on an
  unchanged project the pre-write subtraction emptied the file
  (`"entries": []`) and every accepted finding reappeared on the next
  plain check — a silent wipe whose regeneration run still exited 0.
  Regeneration now snapshots the pre-subtraction, policy-filtered
  diagnostics (suppression comments, severity filter, path-based
  confidence demotion, and `--min-confidence` all apply as in a plain
  check), mirroring the clap conflict at the config level; genuinely
  fixed findings still drop out and the regeneration run reports (and
  fails on) the findings it writes, like the no-config path always did
  (#484).
- The LSP now assembles its multi-file project in one canonical order:
  indexed disk files sorted by path, then open documents sorted by path —
  the CLI's sorted discovery order, with the editor's buffers layered
  last so an open document's definitions shadow same-named on-disk ones.
  The order used to come from unsorted HashMaps and close/reopen
  re-appended the reopened file at the end, so when two files defined
  the same top-level function the winning definition — and with it
  inferred calls and diagnostics — depended on the process's hash seed
  and flipped after closing and reopening an unchanged file (#490).
- The LSP server now applies inline suppression comments and the
  min-confidence threshold before subtracting the baseline, matching
  `ry check`: with two identical diagnostics (same path, code, and
  message) where the first carries `# ry: ignore[...]`, the editor
  could show the unsuppressed twin while `ry check` stayed quiet,
  because the LSP let the suppressed occurrence consume the baseline
  count first (#491). Both frontends now share one post-processing
  pipeline (`ry_checker::post_process`) with a single specified order:
  suppression, severity filter, confidence demotion (CLI only, for
  now), baseline subtraction, min-confidence threshold.
- Honor `base::return(...)` in the walker's `Stmt::Expr` return arm: the
  arm that collects a function's return types and marks the following
  code unreachable matched only the bare `return`/`invisible`
  identifiers, so a qualified `base::return(NULL)` set neither even
  though the block-divergence analysis (#482) already treats it as
  exiting. The arm now recognizes the `::`-qualified callee the same
  way `expr_diverges` does, so code after `base::return(...)` goes
  quiet exactly where it already does after bare `return(...)` and the
  argument's type joins the function's return type;
  `base::invisible(...)` likewise collects its argument type without
  marking the block unreachable, matching the bare form (#510).
- `ry check --watch` on an empty directory used to print the empty
  report and exit 0 without ever entering the watch loop, so the first
  created `.R` file went unobserved until a manual re-run. A
  readable-but-empty discovery result now enters the watch loop with
  zero files instead of exiting: the loop's rescan already detects
  membership growth, so the first created file triggers a re-check. An
  unreadable root still fails the run in watch mode (a discovery
  failure, not a quiet empty set), and non-watch behavior — the
  machine-readable empty report, the stderr note, the exit code — is
  unchanged (#529).
- `ry check --watch` now reacts to non-R inputs without a restart.
  Each poll first re-checks the mtimes of the configuration candidates
  (every `ry.toml` on the upward walk from the search anchor, watched
  even when absent so creation triggers), the effective baseline file,
  every stub file under the configured typeshed directories, and the
  `DESCRIPTION`/`NAMESPACE` files of each watched root and its
  ancestors; on any difference it re-discovers the config, re-merges
  the retained CLI overrides (flags keep winning over `ry.toml`
  edits), rebuilds the severity filter, reloads the baseline and the
  stubs, and coalesces the pass into one re-check — before the R-file
  rescan, so the rescan already runs under a changed discovery config
  (new excludes, fixture flags, index caps). A mid-watch-broken
  `ry.toml` or baseline keeps the last-good inputs with one warning
  per episode (never per-poll spam, never a dead session) and prints a
  recovery note when the file parses again; removing the `baseline`
  key settles to no baseline, matching a fresh run. Serialized `.rda`
  watching stays out of scope (#530).
- Keep watched-file and close-time disk refreshes in the language server
  from reintroducing files full discovery would never admit (#524): the
  per-file admission check matched `exclude` patterns against the file's
  relative path only and never consulted `.Rbuildignore`, so a bare
  `exclude = ["vendor"]` — which prunes the directory entry in the walk —
  never fired for `vendor/defs.R` on the per-file path, and a watched
  create under a build-ignored tree landed files `ry check` omits until
  some unrelated event triggered a full rescan. Admission now runs
  through one shared workspace verdict that replays the walk's per-entry
  rules without walking — excludes against the file and every ancestor
  directory, `.Rbuildignore` with `include_build_ignored` rescue across
  nested package boundaries, the fixed pruned-directory shapes resolved
  at the current package level, and the shared exclude/depth/size thirds
  — so a watched event for an excluded path converges with a fresh
  server instead of disagreeing with it, while an explicit
  `include_build_ignored` override still admits its files.
- Enforce the `index.max-files` budget on the language server's
  incremental disk refreshes, not just on full discovery (#525): watched
  create/change events for files the capped walk omitted used to insert
  unconditionally, growing the index past the configured resource bound
  without limit and letting omitted definitions resolve again. The
  refresh commit now refuses a new entry once the owning root is at its
  budget — first-come-first-served, like the walk — while refreshing an
  already-indexed path still lands and nothing indexed is evicted, so
  the incremental map never holds more per root than a fresh scan of the
  same tree would. The decision happens under the state lock at commit
  time rather than in the blocking admission snapshot, so concurrent
  refreshes racing at the cap serialize instead of each counting a stale
  map.
- Make the language server's per-file disk refresh commits
  generation-safe (#526): a refresh whose blocking read saw older bytes
  used to install its parse over whatever landed meanwhile, so a
  full-scan commit or a second refresh's newer bytes could lose to the
  delayed writer. The refresh snapshot now captures the index generation
  and the commit lands only while it still holds — a stale commit stands
  down without republishing or retiring scans — and a landed refresh
  retires the generation it captured, including a close-time refresh
  retiring an in-flight scan whose walk read the file before the save.
  Deterministic commit-gate test seams pin all three interleavings:
  stale refresh against newer scan, overlapping refreshes with
  last-event-wins, and close-time refresh against an in-flight scan.

## [0.11.0] - 2026-09-16

This release adds five new rules (RY106-RY110) whose founding fixtures are
shipped upstream defects, models `<<-` superassignment and
`formals<-`/`body<-` constructed closures in the binding analysis, and extends
RY000 to R's parser-trust boundary (native-pipe right-hand sides base R
rejects, non-UTF-8 sources). The minimum supported Rust version is now 1.90
for tree-sitter 0.27. Core and the VS Code extension are 0.11.0.

### Changed

- The minimum supported Rust version is now 1.90 (was 1.88): tree-sitter
  0.27.0 and tree-sitter-language 0.1.8 require rustc 1.90. Prebuilt release
  binaries are unaffected; only building from source needs a newer toolchain
  (#466).
- Update tree-sitter from 0.26.13 to 0.27.0 and tree-sitter-language from
  0.1.7 to 0.1.8 (#466, taking over the dependabot update from #465).

### Added

- Add RY110 (`vacuous-all-guard`): `all(is.na(x))` is vacuously TRUE when
  `x` is zero-length, so a validation guard of the shape
  `is.numeric(x) || all(is.na(x))` accepts any empty non-numeric input --
  the pre-fix hms guard behind tidyverse/hms#231, where
  `hms(seconds = character())` passed validation and then failed inside
  `vec_cast()`. Fires only when the vacuously accepted value reaches a
  downstream mode demand a typeshed stub declares (a parameter `type`,
  as RY092 checks) and the guard and demand share the binding (a nested
  closure's same-named parameter never matches); the fix is hms's own
  (`length(x) > 0 &&` before the `all()`). The founding hms shape itself
  is interprocedural -- the guard sits in the `is_numeric_or_na` helper,
  the demand in `hms()`'s unstubbed `vec_cast` -- and stays silent until
  #479 (#462).
- Add RY106 (`ifelse-mode-collapse`): `ifelse()` seeds its result from the
  `test` vector and only overwrites selected positions, so a zero-length or
  all-`NA` test yields a `logical` result even when `yes`/`no` agree on
  another mode -- the typed-NA select behind tidyverse/hms#231, where
  `as.character(hms())` returned `logical(0)` instead of `character(0)`.
  Fires for definite collapses (a literal empty or `NA` test) and for
  typed-NA selects over maybe-empty tests; suggests `vctrs::if_else()`
  (#461).
- RY109 flags formal defaults that reference the formal itself, such as
  `function(x, y, copy = copy)` or `function(n = n + 1)`. The reference can
  only resolve to the promise, so triggering the default errors in R
  ("promise already under evaluation: recursive default argument
  reference") while a supplied argument is unaffected; dtplyr shipped
  exactly this bug (parent commit bffe46e, `R/step-join.R:162` and
  `R/tidyeval-across.R:6,20`, fixed upstream in dbe32a6). ry warns without
  requiring a provable force in the body -- RY098 keeps its stricter
  proven-forcing diagnosis -- because such a default can never evaluate
  successfully; a warning, not an error, because defusing helpers such as
  enquo() can still capture the promise unevaluated. Defaults referencing
  a different formal (`x = y, y = 1L`) are legal and stay quiet, as do
  quoted defaults that capture the formal (`substitute(x)`) (#364).
- RY107 flags scalar comparisons on `any()`/`all()` results, such as
  `any(lengths) == 0` where `any(lengths == 0)` is meant: the scalar is
  compared instead of the elements, so negating comparisons are always
  wrong and constant-outcome comparisons are dead guards. Comparisons
  that preserve the any/all value (`== 1`, `!= 0`, `> 0`) stay quiet,
  keeping idioms like diffobj's `!all(diff(x)) == 1L` clean (#356).
- Add RY108 (`seq-defaulted-forward`): a `seq.*` S3 method that uses its
  defaulted `to` without a `missing(to)` check, behind tidyverse/hms#231,
  where `seq(hms(1), length.out = 3)` returned `hms(c(1, 1, 1))` because
  the unconditional cast and forward treated the defaulted `to = hms(1)`
  as a supplied endpoint. The rule rides a new supplied-vs-defaulted flow
  analysis over formals (`missing()` tests refine `if` arms, early exits
  guard continuations, cached `m <- missing(p)` tests decode like direct
  calls), designed for reuse by the argument rules; `seq.Date`-style
  guards, a `to` without a default, and a `NULL` default stay quiet
  (#463).

### Fixed

- Model superassignment (`x <<- v`, `v ->> x`) from a nested closure as a
  possible type update to the enclosing binding: every `<<-` target in a
  function body (nested closure bodies included) becomes unknown-typed in
  the definition scope once the definition is walked, and a target whose
  rebound root cannot be named (`f()$a <<- v`) discards all value facts.
  The initialization idiom `token <- NULL` mutated only through `<<-`
  inside a callback kept its stale NULL type forever, so the loop/branch
  conditions that R runs fine were flagged -- the vendored json-parser
  family (pak's `R/json.R`: `token <<- tokens[ptr]` in `read_token`,
  `while (token != "}")` in `parse_object`/`parse_array`), flexdashboard's
  `source_file <<- input` in `pre_knit`, jsonlite's `out[[...]] <<- x`
  callback, and curl's `expected[i] <<- ...` download handlers. The
  update lands at the definition's position in the sequential walk: a
  read that precedes the `<<-`-writing definition cannot have seen the
  write at runtime either and keeps its proven type. Reads of a name that
  only ever materializes through `<<-` stop producing RY010, and calls on
  such bindings stop producing RY070; `<<-` never writes the writing
  closure's own frame (R semantics verified: a same-named formal stays
  untouched), which the forwarded-default oracle fixture pins (#374).
- Suppress semantic diagnostics (RY010, RY070, ...) on files whose parse
  produced a recovered tree. Such files report only their RY000 "unparseable
  region" diagnostics, which are the actionable signal: findings derived from
  parser-repaired structure -- including the corpus's empty-name RY010
  (`variable `` is not bound`) -- were noise on top of the parse failure (#380).
- Silence the diagnostics that only exist because a placeholder function
  literal was analyzed as the final closure the replacement-function
  machinery completes: RY010 on names the `alist()`-installed formals
  bind, and -- only under `body(x) <- v`, which discards the walked body
  wholesale -- RY080 on its callback results. `trafo <- function()
  return(x)` followed by `formals(trafo) <- alist(x = )` and/or
  `body(trafo) <- substitute(...)` builds a closure whose formals and
  body ry never sees complete, so walking the placeholder as static
  source flagged names the construction binds at runtime -- distr6's
  genExp trafo and makeChecks assertion builders were 17 corpus false
  positives. Matching is lexical and source-ordered (the replacement
  marks the literal bound to its name at that point in the statement
  list; a rebind ends the association, and `local({...})` argument
  blocks are separate runtime scopes in both directions). A
  `formals<-`-only placeholder keeps RY080 and findings from closures
  nested in its body: only the formals list is swapped, so they survive
  verbatim. `environment(f) <-` grants no opacity; ordinary static
  definitions are untouched (#380, the other half of #467's
  recovered-tree suppression).
- Flag native-pipe (`|>`) right-hand sides that base R's parser rejects
  but tree-sitter accepts, such as `x |> z[.]`, `x |> { ... }`, and
  `x |> sqrt`, as RY000 syntax errors mirroring R's own messages
  ("function '[' not supported in RHS call of a pipe", "The pipe
  operator requires a function call as RHS"). The rewriteable shapes
  stay silent: calls with ordinary heads (including `pkg::f(y)`,
  `"f"(y)`, and `(\(d) d)()`), and extraction chains rooted at the `_`
  placeholder (`x |> _$a`, R 4.3+). The accept/reject boundary was
  verified form by form against R 4.6.1 (#375).
- Flag non-UTF-8 source files (CP1252/Latin-1 bytes) with an RY000 encoding
  diagnostic instead of silently checking them clean, matching R's parser,
  which rejects such files with "invalid multibyte character in parser".
  Invalid bytes inside comments and `%...%` special-operator tokens are
  tolerated exactly like R's lexer (which scans them raw), a flagged file
  reports only its RY000, and the flag flows through the shared read boundary
  so `ry check` and the LSP's on-disk index agree (#376).

## [0.10.0] - 2026-09-15

This release adds three Linux targets, improves package and function lookup,
bounds serialized-data parsing, and refreshes the vendored typeshed to
r-typeshed 0.5.1. Core and the VS Code extension are 0.10.0.

### Changed

- Treat 0.10.0 as the crates.io-publication boundary for the public `Scope`
  collection types (`FxMap`/`FxSet`), first changed at 0.9.2 (#340): no ry
  crate has been published to crates.io at all, so no released consumer has
  seen the `HashMap`/`HashSet` spellings. Rust consumers assigning standard
  maps directly must convert their entries with `into_iter().collect()` or
  use the exposed types (#432).
- Update sha2, serde, toml, and flate2 while retaining tree-sitter 0.26 and the
  Rust 1.88 minimum. The tree-sitter 0.27 update remains separate (#404).
- Document the remaining [scalar-guard limits](docs/scalar-guards.md). The
  assertion/alias/loop flow work remains open (#351).

### Added

- Release binaries and VS Code extensions for 32-bit ARM Linux and Alpine Linux
  on x64 and ARM64. The release matrix now covers nine platforms.
- Explain source discovery with `ry check --explain-files`. Use
  `include-build-ignored` in `ry.toml` to include selected build-ignored files
  in both CLI checks and editor indexing (#363).
- Refresh the vendored typeshed to r-typeshed 0.5.1 (base 0.0.18, tibble and
  vctrs 0.0.2): new export inventories for tibble, scales, readr, checkmate,
  httr, jsonlite, lifecycle, magrittr, stringr, and glue, so wholesale
  imports and bare names from those packages resolve without installed
  copies. Resolving the scales inventory clears the ggplot2 RY010
  false-positive batch from both committed ecosystem ledgers.

### Fixed

- Isolate the bodies of testthat's `test_that()`, `describe()`, and `it()`
  blocks in their own scope, matching their per-test evaluation
  environment. Bindings in one block no longer shadow call heads in
  sibling blocks, removing RY070 false positives in test files (#368).
- Suppress RY020/RY021 for data.table select subscripts such as
  `dt[, -c("col")]` and not-join forms like `dt[!"key"]` or `dt[!list()]`
  when the receiver is not provably a base object. The selector role
  follows the argument tag (`j = `, `.SDcols = `) rather than the
  positional slot, and function or deferred bodies nested in a subscript
  argument (callbacks, `foreach` bodies, data-mask walks) no longer
  inherit the select-form license. Negative or negated
  character subscripts on plain vectors, lists, and base data frames,
  on non-selector arguments such as `drop = ` or a positional `by`, and
  outside subscript positions, still error (#367).
- Resolve `x[i, j]` index arguments against a data.table receiver's columns
  before scope functions, covering data.table's `i`/`j`, `:=` targets, `by`,
  `.SDcols`, and `.SD` pronouns. Unknown columns keep bare names opaque
  instead of borrowing a function's type. Only data.table's `[` masks its
  arguments: receivers classed `data.table` and opaque values (where unstubbed
  data.table calls land). Base data frames, plain lists, and atomic vectors
  evaluate their `[` arguments eagerly, as in R (#369).
- Bundle set6 and dictionar6 export inventories so wholesale imports resolve
  their R6 objects and functions without installed copies of those packages
  (#366). Keep unrelated unknown names reportable.
- Report invalid character conditions through literal assignments and aliases.
  Discard literal facts across calls, forced promises, method effects, writes,
  loops, and branch merges (#425).
- Return RY001 warnings from the checker API, matching the rule table and CLI.
  Keep RY002 length warnings when the condition also contains RY100 (#415).
- Widen parameter defaults on the paths proved by compound `&&` and `||`
  type guards. This avoids false atomic `$` errors for caller-supplied lists
  while preserving errors on paths the guard does not prove (#408).
- Preserve possible function bindings from enclosing frames when an inner
  assignment uses the same name. This avoids RY070 for outer constructors,
  parameters, and unknown values that may be callable (#381).
- Report failed top-level calls even when a later function definition has the
  same name. Preserve outward package lookup and deferred function bodies
  (#410).
- Bind named data arguments before checking data-mask and tidy-select
  expressions, regardless of their position in a call. Quoting helpers without
  a data argument keep an unknown mask (#417).
- Bound serialized-data parser nesting and materialized collection storage,
  including metadata loaded in lazy mode. Keep cycle-safe deduplication and
  the static liblzma build while updating the pinned parser to its 0.2.1 base
  (#412, #433). Over-cap inventories still produce degraded scope notices;
  unsupported or parser-limited files return empty, unflagged inventories.
- Read literal C and C++ routine registration tables so registered symbols
  also resolve through wrappers such as cleancall. Unknown registration forms
  keep the existing fallback (#379). Refresh editor bindings when registration
  sources change.
- Discard forwarded-default facts after writes before wrapped or nested calls,
  including replacement assignments, loop variables, and literal `assign()`
  targets. Match backticked parameter and argument names consistently
  (#407, #409, #411, #423, #424). Preserve local parameter defaults across
  `<<-` and `->>` writes to enclosing bindings.
- Retain provable union lengths through `c()` and one-dimensional subsets of
  classless members, including positive colon indices. Keep class information
  unknown for opaque union members (#405, #416, #434).
- Reject `max-serialized-bytes` values outside 1 byte through 256 MiB. Bound the
  serialized-inventory cache to 1,024 entries and refresh it after same-size,
  same-mtime file replacements, including symlink aliases (#413, #427).
- Report degraded serialized scopes in the language-server log. Honor positive
  `RAYON_NUM_THREADS` values up to eight and parse inline if the index pool
  cannot start (#414, #430).
- Link liblzma statically so distributed binaries run without a system liblzma
  library (#431).
- Build and upload only the selected VSIX registry variant (#419).
- Keep RY105's dead-guard claims on sound length facts: `file.path()` recycles
  its arguments instead of always returning one path — and returns an empty
  vector when any argument has zero length, unlike `paste` (learnr, blogdown,
  pkgload) — and the result length of `seq_len(n)` is the value of `n` rather
  than its vector length (brulee). The pak `length(sum(...)) > 0` true
  positive is retained (#377).
- Honor `length(x) == 1 && ...` guards inside packages, where the bare
  `length` symbol resolves through the package search path and the guard
  exclusion was previously dead code; the 0.9.0 472-package audit measured
  46 such guard warnings, all false positives, and both committed ecosystem
  ledgers drop 14 reviewed false-positive identities of this shape. The
  guard is proven before it is honored: a provably classed parameter, a
  scalar parameter default (which only describes the omitted-argument call
  shape), any project-registered, defined, or imported `length.*` S3
  method, or a reassignment inside the guarded operand keeps the warning
  (#372).
- Demote lexical value bindings under the data.table `[` columns-first mask,
  the value-binding analogue of #369: a column shadows a same-named enclosing
  value at runtime, so `dat[month == "a"]` no longer types the comparison
  against the lexical `month`. Callback formals, `j` argument names, and the
  mask pronouns bind inside the mask and keep their typing (#457).
- Keep `!!!` splice and `!!name :=` injection sites RY021-clean through
  resolvable dynamic-dots constructors: the vendored tibble and vctrs
  inventories declare `injection` metadata for their quos()- and
  list2()-based constructors (r-typeshed 0.5.1). A resolvable entry without
  dots-semantics metadata is strictly worse than no entry — it suppresses
  the unresolved-callee injection fallback — so ggplot2's
  `data_frame0 <- function(...)` forwarder turned five previously clean
  splice sites into false RY021 once the tibble inventory resolved the name
  (found in this release's vendor-sync corpus run).
- Type leading-dot magrittr chains (`. %>% f`, `. %T>% f`) as functional
  sequences: the dot is the chain's parameter, so the chain types as a
  function value instead of flagging the placeholder as unbound, and `%<>%`
  no longer rebinds the placeholder. The native pipe has no such form: `_`
  left of `|>` stays an ordinary unbound name.

## [0.9.2] - 2026-09-10

This release reduces false positives in package code and fixes a crash when
reading serialized package data.

### Fixed

- Keep subset results unknown when the receiver has no known subsetting
  contract. This avoids a false condition-length warning in `rstan`, and
  likewise keeps union receivers such as
  `if (p) c("a", "b") else c("a", "b", "c")` from carrying their
  source lengths past a `[` subset into a false condition-length
  warning.
- Read cyclic serialized objects without a stack overflow, including the
  package data used by `workflowsets`.
- Keep `.data` opaque in data-masked calls such as `aes()` when no usable
  data argument is available. Calls with known data frames still report
  missing columns (#383).
- Correct `mirai::status()` to accept `.compute` and return a list, so `$`
  access no longer produces a false RY061 error (#382).
- Keep class information unknown for opaque stub results without class
  metadata. This avoids false RY092 diagnostics (#341).
- Skip non-function bindings when looking for outward functions at bare
  call heads. Package functions take precedence over dataset names (#384).
- Keep later assignments from invalidating forwarded defaults at earlier
  direct calls (#342).
- Widen default-derived parameter types in branches that reject their
  inferred mode, avoiding false `$` errors in those branches (#343).
- Accept scalar character, raw, and complex conditions that R can coerce
  to logical. Known invalid literals still produce RY001. Multi-value
  conditions are newly visible as RY001 warnings in default output:
  numeric conditions previously carried only the default-off RY003
  nudge, and multi-element logical loop conditions were not reported
  (#373). Review baselines and `--error-on-warning` runs for new
  findings.
- Report files whose parser cannot be initialized instead of panicking the
  whole check, and fall back to serial indexing in the language server when
  its parse pool cannot be built.

### Changed

- Raise the default serialized-data cap from 2 MiB to 16 MiB. Packages such
  as gt can resolve their internal data names without custom configuration.
  Larger files still produce a degraded-scope notice (#378).
- Reduce binary size with symbol stripping, full LTO, and compressed
  embedded stubs. Load package stubs when needed (#340).
- Speed up workspace indexing and project checks with bounded parallel
  processing and cached package discovery (#340).
- Hash checker-internal identifier maps with FxHash instead of SipHash.
  `Scope`'s public collections (`bindings`, `parameter_bindings`,
  `function_aliases`, and siblings) are now `FxMap`/`FxSet` rather than
  `HashMap`/`HashSet`; a source-level change for library consumers (#340).
- Clarify RY001 and RY070 messages. Baselines match message text, so
  previously accepted findings can reappear after upgrading. Review them
  before regenerating the entries you still accept.

### Editors and maintenance

- Align the core and VS Code/Positron extension at 0.9.2. The extension uses
  a smaller production bundle and reuses successful binary-version checks
  during the same editor session; see its [changelog](editors/code/CHANGELOG.md).
- Use `scholzmx.ry-checker` for the VS Code Marketplace listing and keep
  `scholzmx.ry` for Open VSX. Prepare `ry-lsp` as the Zed gallery identity.
- Update R setup, Pages deployment, and workflow security Actions.

## [0.9.0] - 2026-09-07

### Added

- Embed verified ggplot2 signatures for 498 functions, 145 exported values, and
  11 lazy datasets. Curated capture metadata now owns aesthetic evaluation;
  ordinary helpers such as `aes_string` no longer suppress unbound arguments.
  Complete base matrix/array construction and row/column summary formals.
  Preserve unknown result shapes for polymorphic `regmatches`, `sort`, and `sort.int`
  calls instead of assuming character or double vectors.

- Export reference identities across ordinary literal/copy reassignments and
  retain proven reads before opaque statements. The schema-2 reference
  capability is now `same_file_ordered_prefix`; coverage remains partial.

- Capture the argument read in standalone proven `base::length(x)` calls.
  Preserve local/formal identity at that read and keep the post-call suffix
  unsupported, including fresh assignments.

- Use reviewed typeshed forcing contracts for RY098. Qualified calls to
  `typeof()`, `length()`, `is.null()`, `is.function()`, and `invisible()` can
  expose recursive defaults or force defaults before local assignments. Calls,
  other promise reads, and possible binding replacements stop attribution of
  later reads to the original default. Subscript promises remain lazy across dispatch.

- `ry dump-facts` exports versioned structured types, scope-exit snapshots,
  UTF-8 source spans, and analysis context hashes for downstream tools.
  Add `--references` for conservative same-file reference facts, explicit
  resolution status, and source definition IDs in schema 2.
  See the [facts schema](docs/facts.md). Existing `dump-types` output is unchanged.

- **`ry dump-types` command**: `ry dump-types <FILE>...` runs the same
  analysis pass as `ry check` and prints recorded lexical scopes of the
  requested files as JSON on stdout: scope kind, name, and extent, plus
  each binding's name, kind (`param`/`local`/`closed-over`/`imported`),
  type string (the same rendering the editor's inlay hints show, `unknown`
  when inference has nothing), and definition site. `--position LINE:COL`
  (repeatable) restricts output to the innermost recorded scope containing each
  position and drops locals assigned after it. `--project-root <DIR>`
  overrides the analysis root for non-package files; the default mirrors
  `ry check`'s per-package (DESCRIPTION) grouping. The exit code is 0 even
  when the analyzed code has diagnostics; non-zero means a usage, IO, or
  internal failure.

- **Bounded file discovery**: `index.max-files` (default 20,000),
  `index.max-file-bytes` (default 2 MiB), and `index.max-depth`
  (default 64) limit how many files `ry check` and the language server
  discover. Each accepts a positive integer; zero is a configuration
  error. Hitting a cap produces one warning per scan in the editor and a
  CLI warning.

- **Identical file sets in CLI and editor**: `ry check` and the language
  server share one directory-discovery engine, so both see the same
  project, including hidden, excluded, oversized, deeply nested,
  symlinked, and test-fixture files.

- **VS Code / Positron extension** (`editors/code/`): installable from the
  VS Code Marketplace and Open VSX. Bundles the `ry` binary, exposes the
  `ry.lint.*` settings, and supports both `fromEnvironment` and
  `useBundled` import strategies.

- **Zed extension** (`editors/zed/`): locates the `ry` binary via
  settings, `PATH`, a previous download, or a fresh GitHub-release
  download, with path construction unit-tested for all six cargo-dist
  targets. Settings are validated (`minConfidence` must be `low`,
  `medium`, or `high`).
  Zed verifies downloaded server executables against published SHA-256 sidecars
  and rechecks cached binaries before starting them. Missing or invalid sidecars
  and mismatched binaries fail installation and remove the download directory.
  Automatic downloads require ry 0.9.0 or newer; settings and PATH overrides
  remain available for user-managed binaries.

- **Matching diagnostics in CLI and editor**: `ignore`, `select`,
  `extend-select`, `error`, `warn`, `exclude`, `baseline`,
  `min-confidence`, default-disabled rules, package metadata, and Unicode
  positions produce the same codes, severities, messages, and locations
  in `ry check` and in the editor for a single workspace root.

- **`ry.toml` hot-reload**: editing `ry.toml` updates diagnostics without
  restarting the language server.

- **Multi-root workspaces**: per-folder `ry.toml` configs are honoured.

- **`ry server --log-level`**: configurable server tracing on stderr.

- **Documentation**: `docs/editor-defaults.md` collects evidence-backed
  editor-safe settings, and `docs/release-runbook.md` documents the
  binary, VS Code, and Zed release processes.

### Changed

- Use the published tree-sitter-r grammar instead of a vendored patch. Valid R code
  with spaces, newlines, or comments between double-subscript closing brackets
  (such as `x[[i] ]`) is a known parser limitation again; use `x[[i]]` instead.

- Move detailed configuration, usage, rule, and inferred-type references from
  the README into linked guides, including `docs/types.md`. The README now
  focuses on installation and first-use examples.

- **More accurate `if`-condition nudges**: the "non-empty check" idiom
  (`if (length(x))`, `if (nrow(df))`, ...) is now recognized from the
  function's declared return type instead of a fixed name list, so it
  covers every function that returns a count which can never be `NA`
  (`nobs`, `vec_size`, ...). `if (Position(...))` is deliberately NOT
  treated as that idiom anymore: when nothing matches, `Position()`
  returns `NA`, and R errors on the condition instead of testing
  non-empty — you now get the coercion note there, same as for
  `if (1L)`. A local binding that shadows the called name (including a
  locally defined `is.*` predicate feeding `sum(...)`) no longer
  inherits the idiom credit; an aliased base predicate does.

- **List-valued results are tracked by inferred type**: which values
  count as list-shaped now follows the inferred mode instead of a
  fixed name list, so the roughly 90 functions that return a list
  (`strsplit`, `split`, `read.table`, ...) keep that fact through
  renames and re-assignment — in blocks and `if` expressions too, not
  only direct calls. This feeds RY101's always-FALSE check for
  `identical(x[1], "scalar")` on list subsets.

- **Quoting helpers are known from their signatures**: base
  `quote`/`substitute`/`bquote`/`expression`/`delayedAssign` and rlang
  `expr`/`exprs`/`quo`/`enquo` now suppress unbound-variable warnings
  only for the arguments they actually quote. Two visible
  consequences: `delayedAssign` no longer stays silent about its other
  arguments, and rlang's ordinary evaluating helpers (`sym()`,
  `abort()`, `inform()`, `new_formula()`, `new_quosure()`) now report
  undefined names passed to them — R reads those arguments, so
  `abort(undefined_name)` was always a bug. `rlang::quo()` with no
  argument is also understood to create an empty quosure instead of
  reporting a missing argument.

- **Package test files see the package's imports**: testthat runs
  `tests/testthat/` inside a copy of the package namespace, so names
  supplied by a wholesale `import(pkg)` directive in NAMESPACE are
  available there. `ry check` now models this: bare `expr(...)`/
  `quo(...)` calls in a package's testthat files no longer report
  unbound variables when the package imports rlang, matching how the
  same call in `R/` was already treated. Outside that context (no
  attachment and no import), a bare unattached `expr(undefined)` still
  reports the name like any other unknown call.

- **Backtick-bound top-level values resolve from functions**: a bare
  read of `n1` after `` `n1` <- 42 `` no longer reports RY010. This also
  covers functions used as values. Escaped identifier spellings remain
  conservative, and string assignment targets retain their literal names.

- **Names inside quoted blocks cannot borrow unrelated function
  types**: inside an unevaluated block (a data-mask argument, or code
  quoted for later use), a bare name that matches nothing locally used
  to resolve to a same-named base function — so a data-mask column
  called `class` behaved like the `class()` function in a comparison,
  and a logical formal like `append` inside withr-style deferred code
  behaved like `append()`. Such names now type as unknown inside those
  blocks, removing spurious condition and comparison warnings
  (RY001/RY030) there, while genuine argument mistakes inside the same
  blocks are still reported.

- **JSON diagnostics no longer suggest fixes**: the `fix` payload is gone
  from `ry check --output-format json` and from the `data` field of
  published editor diagnostics. Nothing ever applied these suggestions —
  there is no `ry check --fix`, and the editor's quick-fix actions only
  insert suppression comments — and a replacement that is correct in
  isolation can be wrong under R's non-standard evaluation. Diagnostics
  are otherwise unchanged: codes, spans, messages, severities, and
  confidences are identical. No shipped release ever contained the `fix`
  field; where autofix should live is tracked in #89.

### Fixed

- Respect S3 `length()` dispatch and uncertain class metadata before reporting
  zero-length guards as constant; classless scalar bindings retain RY105.

- Avoid claiming atomic `sapply()`, `mapply()`, and `tapply()` results when
  simplification is disabled or uncertain, controls are forwarded through
  `...`, or inputs may be empty. Scalar simplification requires a matched,
  enabled control and a provably nonempty input.

- Suppress RY093 for proven base `grep()` position comparisons used directly
  as boolean guards, while retaining warnings for value results and uncertain
  or nested comparisons.

- Distinguish `@` slot extraction from `$`. Valid atomic `.Data` reads no
  longer trigger dollar-access errors. Keep slot results and replaced roots
  unknown, including mixed nested replacements; respect explicit accessors.

- Keep `Find` results unknown when no-match values or matching list elements
  can have arbitrary types and lengths. Preserve predicate diagnostics.

- Keep `Filter` subset results and `Position` no-match values unknown instead
  of borrowing input types or assuming scalar indices.

- Capture bare component names in `stats::model.extract` without reporting
  them as unbound variables; keep ordinary frame arguments checked.

- Account for forwarded `...` when checking missing and unknown arguments.
  Expanded arguments can fill required parameters and resolve partial names;
  explicit named holes and unrelated argument names still produce warnings.

- Keep `grep`, `confint`, and fold results conservative across their supported
  return shapes. Complete `grep` and `confint` formals; character grep results
  and list-valued confidence intervals no longer cause false type errors.

- Honor tidy-evaluation injection in `ggplot2::aes` aesthetics and `vars` facets,
  avoiding false negation errors for unquoting and list splicing. Ordinary
  helper arguments continue to execute R negation.

- Forget stale receiver types after `storage.mode(x) <- ...` and `mode(x) <- ...`.
  Coercing a character or list value no longer leaves arithmetic checking its
  previous storage type; the assignment expression still returns its right side.

- Match `rapply` result controls through the full R argument match, including
  positional and partial `how` arguments. Keep recursive unlisting and dynamic
  modes unknown; preserve outer list shape only for proven list/replace calls,
  including the retained outer class for replace mode. Sync the verified opaque
  `rapply` source contract from base revision 0.0.10.

- Math and Summary member calls no longer emit RY050 just because an unrelated
  class has a local group method. Built-ins such as `sum()` and `abs()` can
  use their default behavior without a class-specific method. The Math
  inventory includes cumulative functions, `signif`, the `*pi` functions,
  `digamma`, and `trigamma`, with recognition of their specific methods.

- `ry check` now emits an empty JSON/GitLab array or JUnit report when no R
  files are discovered, including when configuration excludes every source.

- Report an explicit nesting-limit error before deeply nested syntax can
  overflow the parser stack. The limit is 128 tree-sitter syntax levels.

- Decode adjacent high/low Unicode surrogate escapes as a single UTF-8 scalar,
  while retaining raw recovery text for unpaired or malformed surrogates.

- Decode octal and braced Unicode string escapes, escaped spaces and backticks,
  and UTF-8 byte sequences correctly. Preserve physical escaped newlines and
  retain raw recovery text for malformed or unrepresentable string values.

- Keep classed atomic `$` reads and writes conservative when an S3 method may
  handle the access, including classes attached to a union of payload types.
  Discard caller facts that the method could change.

- Report missing format arguments only for proven base `sprintf` and
  `gettextf` calls, avoiding false RY094 warnings for custom functions.

- Require a matching receiver class before inferring a registered S3 method's
  return type. Keep uncertain dispatch opaque instead of borrowing a method or
  scalar default return from an unrelated class.

- Add primary blocker provenance to reference facts, locating statement,
  ancestor, declaration, and unsafe-read restrictions while preserving existing
  resolution statuses, reasons, and unavailable evidence.

- Stop applying `hasArg` and `on.exit` deferred semantics to explicit competing
  bindings, formals, and aliases. Retain existing inference under ambient
  lookup uncertainty, while requiring methods provenance for RY096.

- Recognize `.Generic`, `.Method`, and `.Class` inside subset and subset
  replacement methods, avoiding false undefined-variable warnings.

- Compute data-frame column types after scalar arithmetic instead of copying
  their input types. Keep classed column results unknown when methods may run.

- Infer double results for primitive division and powers of integers. Reject
  complex remainder and integer division only when both operands are nonempty.

- Match `structure()` payloads through `.Data`, preserve class and list-column
  information for resolved base calls, and evaluate class attributes. Respect
  shadowed constructors and class-vector builders; discard stale column names
  after name attributes change.

- Refresh incremental diagnostics when a callback changes, including callbacks
  passed as values and their downstream callers.

- Reject suppression requests that reuse stale diagnostics when the editor
  supports preserving diagnostic data.

- Stamp suppression edits with the analyzed document version when the editor
  supports versioned edits, so it can reject actions after the source changes.

- Stop offering suppression actions for disabled or excluded files. Skip
  computing them when the editor requests other action kinds.

- Stop S3 operator lookup at the winning group method, avoiding false
  column-access errors when later classes define another operator method.
  Keep custom opaque method results unknown.

- Wait for workspace bindings before publishing initial editor diagnostics,
  including files opened while the workspace scan is running.

- Show editor type hints from each assignment, including function locals,
  rather than applying the file's final binding type to earlier assignments.
  Refresh cached hints when local stubs change.

- Match promise-capture helpers by formal argument when collecting wrapper
  evaluation modes. Recognize qualified base helpers and keep their environment
  arguments, and explicit rlang controls, separate from captured expressions.

- Avoid recursive-default warnings for signaling arguments that may be ignored
  or evaluated conditionally.

- Avoid RY098 warnings for body-local names captured by qualified `base::quote`,
  `substitute`, `expression`, and `rlang::expr` calls in defaults. Keep
  checking evaluated control arguments and tidy-injection payloads.

- Resolve the exported `htmltools::tags` list and shiny re-export under
  ordinary namespace/import lookup, avoiding unbound-name warnings for tag
  constructors without adding an ambient global.

- Keep `expand.grid` results conservative, so dropped numeric columns and
  data-frame arithmetic do not inherit the plain-list storage type. Include
  its exact `KEEP.OUT.ATTRS` and `stringsAsFactors` control names.

- Resolve S3 operators before inferring data-frame results, so subclass methods
  can return other types and conflicting methods do not retain column schemas.

- Validate typeshed updates before replacing the vendored snapshot. Failed
  validation leaves the existing stubs and provenance intact. Restore the old
  snapshot if installation fails or receives a handled interrupt, and retain a
  recovery copy if restoration fails. Refresh the embedded provenance timestamp
  after validation so the next Cargo build includes the installed snapshot.

- Infer vector-constructor lengths from size values, including empty defaults
  and fractional sizes. Correct factor arithmetic with NULL and unary minus.

- Check typed purrr callback contracts independently of output-length inference.
  RY080 now reports incompatible results as errors; empty inputs stay silent.

- Preserve ordinary double negation in known evaluation contexts and
  propagate splicing and data-mask behavior through wrappers and S3 methods.
  Keep unresolved callables conservative about argument capture.

- Discard initial values for bindings reassigned inside loops, preventing
  stale lengths and types from being applied to later iterations. Preserve
  bindings at `break` and `next`, and exclude later unreachable writes from
  loop exits. Nested loops and function bodies keep separate exit states.

- **`enable` is honored per folder**: a workspace folder whose settings
  set `enable: false` is skipped: the language server publishes no
  diagnostics and returns no inlay hints for it. The setting was
  accepted and ignored before. The server also stops modeling settings
  it never read; unknown settings keys remain ignored.

- **Rule table restored in `docs/rules.md`**: RY003, RY102, RY103, and RY105 are
  listed again, with a note that RY003 is default-off, and an automated
  check now fails if the table and the rule registry drift apart (#107).

- Detect recursive defaults forced by `TRUE && x` and `FALSE || x`, including nested operands.

- Detect recursive and prematurely forced defaults passed to `base::identity` or
  `base::force`, while preserving laziness in quoted, masked, and conditional calls.

- Avoid RY032 warnings when a parameter is only the lookup table for `%in%`,
  or a base `length(x) == 1` guard protects a scalar predicate.

- Correct RY002 and RY032 explanations: R rejects conditions and scalar
  logical operands with more than one element.

- Avoid RY098 warnings for recursive names in default expressions when literal
  `if` conditions or short-circuit operators skip their evaluation.

- Select only the executed alternative for proved base `switch` calls with
  literal scalar selectors, preserving missing fallthrough and caller assignments.
  Dynamic selectors and custom-call argument laziness remain outside this model.

- Correct typed purrr multi-input map results and remove an unsupported scalar-length fallback. Await the mirai oracle result before shutting down its daemons.

- Watch custom editor configuration paths and reload settings when the path changes, retaining the last valid configuration after malformed edits.

- Re-resolve the VS Code server on restart and retain the working server if its replacement fails. Verify the installed VSIX in trusted and untrusted workspaces.

- Suppression quick fixes preserve existing comments and rule lists, ignore
  diagnostics from other tools, and refuse line edits inside multiline tokens
  or malformed source. File suppression preserves a script's shebang.

- Reject duplicate function keys and S3 dispatch definitions when loading
  typesheds. Function key ordering no longer produces a validation warning.

- CLI and LSP use the same UTF-8/Latin-1 source decoder for on-disk R files.

- Environment profile paths use anchored glob matching, preventing bindings
  from leaking into directories with similar names.

- Serialized workspace files stream through the decoded-byte limit instead
  of allocating the complete input before checking the limit. The setting
  continues to limit decoded bytes, so compression overhead does not reject
  an otherwise valid exact-cap payload.

- `ry typeshed validate` rejects duplicate formal names, including repeated
  `...`, in function and S3 method signatures.

- Suppression quick fixes use the cached document parse. File-level actions
  recognize case-insensitive directives and ignore markers inside strings or
  prose comments; line-level detection retains multiline string context.

- Typeshed validation rejects empty recycled-value parameter sets, mismatched
  callback names and positions, and unsupported conditional-scope values.
  Return-length rules also reject unknown or misplaced control fields.
  Higher-order result indices (`length_arg`, `source_arg`,
  `template_position`) must identify a formal parameter, so custom stubs with
  out-of-range indices that previously loaded silently now fail validation.
  Custom stubs with missing required controls or unknown return-length fields
  now fail to load; schema 2's documented control shapes remain unchanged.
  Update purrr stubs with corrected callback positions and `walk2` callback
  arguments, plus missing control parameters used in argument matching.

- Invalid config reloads retain the language server's last valid settings.
  A missing explicit configuration path is also a load failure; removing an
  automatically discovered `ry.toml` restores ancestor settings or defaults.

- Typeshed loading and validation now discover mixed-case nested filenames,
  including `rcpp/Rcpp.json` and `s7/S7.json`.

- **VS Code Explain Rule**: use the CLI's actual rule-list command and display
  its summary without a second subprocess. Binary paths are passed literally,
  and document-opening errors reach the command's error message.

- **testthat runner classification follows the documented contract**:
  under `tests/`, only `.R`/`.r` files directly at the root (what
  `R CMD check` sources, including `tests/testthat.R`) and, under
  `tests/testthat/`, `test-`/`test_` test files plus
  `helper`/`setup`/`teardown` files classify as executed code. Prefix
  lookalikes such as `testing.R` and legacy S-dialect spellings
  (`.S`/`.s`/`.q`) anywhere under `tests/` are fixtures — skipped
  unless `check_test_fixtures` is enabled (#174).

- **Operator S3 dispatch sees the same methods as calls** (#165):
  `x + 1` now resolves `+.foo`/`Ops.foo` through the same source ladder
  as `+(x, 1)` — external registrations, project functions, base and
  package typesheds. A miss is silent, as in R (the primitive is the
  fallback): no RY050, and `+.default` is never consulted as a
  fallback. `&&`/`||` never dispatch through `Ops`, so their
  RY031/RY032 diagnostics cannot be hidden. When the default `Ops.factor`
  behavior applies, factor arithmetic warns RY042 even for a list counterpart
  instead of erroring RY040, without a false RY041 recycling warning. When
  operands resolve to different methods, ry no longer assumes the left method wins. Top-level literal
  methods and `chooseOpsMethod` values can prove selection, including aliases
  and reverse selection. Uncertain dispatch stays unknown; remaining
  `chooseOpsMethod` support is tracked in #193.

- **`bquote` quotes unquotes inside braced bodies**: a `.(x)` in
  `bquote({ 1 == .(x) })` was not recognized as quoting, so the
  argument passed at the call site was treated as eagerly evaluated and
  an unbound name there got `RY010` (`unbound-variable`). Braced bodies
  now get the same unquote scan as the rest of the template.

- **`-vv` now enables trace logging**: the CLI mapped every verbosity
  level above `-v` to `ry=debug`, so the trace tier promised by the help
  text never activated. `-vv` and higher now set `ry=trace`; `-v` and the
  quiet flags are unchanged. The help text also claimed `-v` selects
  debug; it now says info, matching the filter `init_tracing` applies.

- **Oversized-file warning no longer contains stray spaces**: the
  `index.max-file-bytes` warning printed a wide run of stray spaces
  inside the sentence. The message now uses single spaces.

- **Corrected garbled messages**: RY032's `||`/`&&` operand-length
  warning now ends "R errors at runtime for vector operands" instead of
  "current R errors for vectors"; the `ry dump-types --format` error
  says "only `json` is supported" instead of "expected one of: json";
  and `ry check` with no R files prints its search roots with normal
  path formatting instead of Rust debug output.

- The language server no longer panics on every later check when a worker
  thread panicked once: the serialized-workspace cache recovers from a
  poisoned mutex instead of propagating the panic.

- RY010 now fires for arguments that bogus or redundant hardcoded NSE
  entries used to suppress: calls spelled `tidyselect(...)` (a package
  name, not a function), rlang defusing helpers (`enexpr`, `ensym`,
  `enquo`, `enquos`, `ensyms`, `quos`) called unqualified without
  `library(rlang)`, and `all_vars` called unqualified without
  `library(dplyr)`. Loaded or qualified calls keep their stub behavior:
  the rlang helpers capture their arguments, and `dplyr::all_vars`
  data-masks its expression.

- Parsing no longer panics when a string literal ends inside a multi-byte
  UTF-8 character.

- Re-running the checker on a single file no longer leaks inference state
  (functions, known variables) from the previous run into the next, so
  diagnostics no longer accumulate across files.

- The VS Code extension's language server now uses the binary path
  resolved by the extension itself, and untrusted workspaces can no
  longer execute arbitrary binaries via checked-in `ry.path` settings.

- **Corrected rule table in `docs/editor-defaults.md`**: RY020, RY030,
  RY040, and RY090 now carry their registry names (`unary-minus-type`,
  `invalid-comparison`, `invalid-arithmetic`, `unknown-argument`). RY032
  is documented as the enabled `scalar-logical-length` warning it is, with
  its measured 1 TP / 47 FP, instead of a disabled "test fixture" rule.
  RY003 is documented as the only default-off rule. The baseline-findings
  table now points at `docs/corpus/0.9-release-evidence.md` instead of
  duplicating it, and the drift check that guards the `docs/rules.md` rule table
  (#107) now also guards this table.

- Preserve omitted call arguments and their names without shifting later
  arguments. Calls and indexes now share missing-position handling. The public
  `ry-core` AST adds `Expr::Missing(Span)`; consumers with exhaustive expression
  matches must handle it separately from unsupported `Expr::Unknown` forms.

- Keep fold accumulators and results conservative in `Reduce()` and
  `purrr::reduce()`, while retaining element checks for known directions.

- Custom or masked `factor` and `new` calls no longer acquire builtin constructor facts. S4 constructor inference requires methods provenance, and detaching a package invalidates the default search-path assumption.

- Resolve visible custom arithmetic, comparison, and vector logical operators
  before their operands. Avoid primitive diagnostics for ignored operands and
  preserve proven constant returns; discard caller facts after uncertain effects.

- Class assignments no longer infer literal classes from custom class builders or preserve payload types under an unproven replacement function.

- Report RY051 when literal `chooseOpsMethod` results are `FALSE` on both
  sides and reject provably distinct operator methods, including methods
  with unequal literal bodies of the same storage mode. Infer primitive
  fallback and its class behavior only for proven scalar operands.

- Extend explicit `FALSE`/`FALSE` Ops chooser fallback to plain vectors built
  with proven base `structure()` and flat literal `base::c()` payloads.
  Arithmetic keeps the longer operand's class (left on ties); comparison and
  logical results drop it. Attributes, unknown lengths, and empty constructors
  remain outside this proof.

- Parse exponent and hexadecimal integer literals with their values, and use
  double storage when an `L`-suffixed value exceeds R's integer range.

- Give CLI commands short summaries in `ry --help`; keep scope-dump and language-server details in their command help.

### Removed

- **Slimmed language-server capabilities**: the server no longer
  advertises `textDocument/rename`/`prepareRename`,
  `textDocument/documentHighlight`, `textDocument/foldingRange`,
  `textDocument/selectionRange`, `textDocument/hover`,
  `textDocument/definition`, `textDocument/references`,
  `textDocument/documentSymbol`, `workspace/symbol`,
  `textDocument/completion`, and `textDocument/signatureHelp`. Rename,
  highlighting, and navigation resolved identifiers purely by spelling,
  which is unsafe in R (NSE, `assign()`/`get()`, S3 dispatch by naming
  convention, `$` on lists/environments, formulas, and `library()`
  masking). Folding, selection ranges, outline, and symbol search
  duplicate what every tree-sitter-based R editor integration already
  provides. Cross-file hover, definition, references, completion, and
  signature help never worked as shipped (requests collapsed to empty
  ranges). Real completion and signature help belong to dedicated R
  editor integrations. Navigation and rename remain deferred feature ideas
  in #88; revisiting them would require real cross-file symbol resolution
  and a decision to expand the checker-focused scope in #87.

- **Remaining language-server surface**: exactly `textDocument/inlayHint`
  (the checker's output rendered inline) and `textDocument/codeAction`
  (inserting suppression comments), both scoped to open documents. The
  background file index stays, so published diagnostics continue to merge
  on-disk files with open documents and the editor sees the whole project
  exactly as `ry check` reports it. Version 0.8.0 advertised the removed
  capabilities, so upgrading reduces the server capability set. Use another
  R editor integration for navigation, completion, signature help, and rename.

- **`r-version` config key**: the no-op key, reserved for future use and
  accepted but ignored, is gone. `ry.toml` files that still set it now
  fail config parsing (`deny_unknown_fields` rejects unknown keys), so
  delete the line when upgrading.

- Removed the nonfunctional VS Code setting `ry.checkTestFixtures`. Set
  `check-test-fixtures = true` in `ry.toml` to enable fixture checks.

### Performance

- Keep only promise-capturing functions in the collection index, reducing startup
  allocations without changing capture lookup results.

- Refine only affected functions after edits, using observed callable reads and
  forwarding or S3 metadata dependencies. Keep diagnostic invalidation conservative.

- Skip name hashing when assignments invalidate empty scope metadata tables.

- Journal statement `if` mutations instead of cloning scopes for both arms,
  preserving inference while reducing allocations on nested branches.

- Reduce scope copying for assertions and short-circuit expressions.

- **One pass-1 walk per file, syntax-only attachment harvest**: the
  collection pass now harvests each file's `library()`/`require()`
  attachments in the same walk that collects its function definitions,
  replacing a second full discarding inference walk per file (#178).
  The syntactic harvest drops the rare alias indirection `lib <-
  library; lib(dplyr)` but counts attachments after code the walker
  proves unreachable (past a `stop()`); diagnostics and inferred types
  are otherwise unchanged.

- **Less work per `if` during checking**: a condition that proves no type
  refinement skips the narrowing machinery, and merging branch bindings no
  longer copies the branch scopes. Diagnostics and inferred types are
  unchanged.

- **Less duplicate work per call and per function entry**: a call site now
  matches its arguments against the callee's formals once instead of once per
  argument query, function bodies are entered through a single walker path,
  and the RY098 defusing-helper set is built once per collection round
  instead of per function literal. Diagnostics and inferred types are
  unchanged.

- **Fewer redundant editor updates**: when a check pass leaves the
  workspace environment unchanged, the language server no longer re-emits
  every file's diagnostics; a genuine change still invalidates the whole
  project (#86).

- Editing no longer forces a full project re-collection on every
  keystroke; the removed workaround did not prevent the failure it was
  added for.

### Maintenance

- Check corpus package totals against their reviewed findings to catch stale
  summary counts in CI.

- Replace the unmaintained xz2 bindings with liblzma for compressed R data.
  Pin rds2rust to a tested fork revision with the same dependency switch.

- Split inference, workspace discovery/serialization, and LSP handlers into
  focused modules. Avoid allocating names and scanning infix methods on ordinary calls.

- Validate editor JSON responses, remove unsafe type assertions, and keep rule
  lookup asynchronous so it does not block the extension host.

- **Checker and CLI internals consolidated**: the typeshed-resolution
  ladders share one attached-package lookup per attachment gate (#166),
  and `ry check`'s orchestration moved out of the CLI entrypoint into
  `check.rs` (#182); the last owned `collapsible_if` debt was lifted
  (#185). Diagnostics, inferred types, and CLI behavior are unchanged.

- **Remaining `collect.rs` walkers on the shared walker**: the
  parameter-use collector, the declared-globals scan, the
  function-definition collection, and the nested-definition collection
  now express their traversal through the shared `ry_core` walker
  (`Walk::ALL` for the first two; a statement-level policy that skips
  control tests for the definition walks) instead of four hand-rolled
  Stmt/Expr recursions. The `first_parameter_use` family stays
  hand-rolled: it answers a first-use query in evaluation order whose
  rules select individual children (the value side of a complex
  assignment before its target, both `if` branches past their first
  hits, the `for` re-binding between iterator and body), not whole
  subtrees. Every converted walker ships with a test pinning its skip
  policy; diagnostics and inferred types are unchanged (#163).

- **Shared test harnesses, leaner comments, `suppress.rs` renamed to
  `resolve.rs`**: the checker's inline tests gained `check_with` (parse,
  configure, check) and a shared `parse_file`, replacing copy-pasted
  parser/checker scaffolding; the language server's test binaries share
  one harness module (`tests/harness/`) for spawning sessions (with or
  without client capabilities and `initializationOptions`), incremental
  edit splicing, and the `Published` normalization used to compare LSP
  and CLI diagnostics item by item; near-duplicate literal-pair tests
  are table-driven. Narrating comments that restated the next line were
  removed. Contributor-facing rename: `crates/ry-checker/src/suppress.rs`
  is now `crates/ry-checker/src/resolve.rs` (same code; it holds the
  typeshed/package signature and value resolution plus the checker's
  emit helpers). No behavior change.

- **One shared front half for `ry check` and `ry dump-types`**: both
  commands resolve their per-package groups, workspace contexts, and
  checker inputs through one pipeline helper, so their file sets,
  resolution roots, and degraded-scope notes cannot drift apart.
  Diagnostics and dump output are unchanged.

- **Consolidated duplicated helpers across the crates**: `ry rule` and
  `ry explain rule` share one argument struct; `ry.toml` merging takes a
  single `CliOverrides` value instead of ten positional flags; the
  checker's argument matching, condition inference, and plain-assignment
  binding each have one implementation; the language server partitions
  open documents per folder once, carries the owning folder through
  publication, and reads the parse cache under a single lock; workspace
  resolution caches DESCRIPTION reads per package root. Tests for
  workspace discovery, `.Rbuildignore` translation, and baselines moved
  into the crates whose code they exercise. Behavior, diagnostics, and
  inferred types are unchanged.

- **Dead feature and API-surface sweep**: the AST's statement-position
  `function(...)` literal loses its never-populated `name` field (named
  functions lower to assignment form), the checker's write-only
  `vector_intent_parameters` stack is gone, and `Project::add_file_arc`
  replaces the deep `SourceFile` clones the CLI and the benchmark made
  just to re-wrap each file in an `Arc`. Public surface trimmed:
  ry-workspace's `PackageFileKind`/`package_file_kind` (now an internal
  predicate that classifies the same paths as test fixtures),
  `TruncationReport::omitted_count` (the adjacent per-file loop already
  reports oversized files precisely), `SeverityFilter`'s raw token
  buckets, ry-checker's unused re-export of the package file kinds, and
  six unused `FixtureProject` builder methods. The CLI drops its unused
  `thiserror`, `toml`, and `glob` dependencies, and the checker its
  unused `thiserror`. Tests that duplicated another test or could not
  fail were deleted rather than kept as theater; diagnostics and
  inferred types are unchanged.

- The test harness's async JSON-RPC decoder now applies the same 16 MiB
  message cap as the blocking decoder, rejecting oversized headers instead
  of buffering without limit.

- VS Code extension publishing: fixed the duplicate `needs: version` key
  in the release workflow, made its version and core-tag inputs explicit,
  replaced the empty pull-request build workflow with a required one, and
  standardized the publisher identity to `sims1253.ry`.

## [0.8.0] - 2026-08-04

This release focuses on checker precision, higher-order R semantics, and editor
correctness. It is a minor release because diagnostic output and inferred
warning sets intentionally change, and JSON diagnostics gain a new field.

### Checker and type inference

- Higher-order calls such as `Map()`, `mapply()`, `vapply()`, `Filter()`, and
  purrr map-family functions now bind callbacks, sources, templates, and
  controls by R formal-argument matching rather than raw call position. Named,
  reordered, and partially matched arguments therefore infer consistently.
- Single-bracket vector and list subsetting derives result length from logical
  and numeric indices when it is provable. Literal negative exclusions retain
  exact length for known inputs, while transformed subsets no longer carry
  stale source-column schemas.
- Recursive parameter defaults (`RY098`) distinguish guaranteed forcing from
  quoting, conditional paths, short-circuit evaluation, loop reachability, and
  replacement assignments. This removes false positives while preserving
  provable recursion diagnostics.
- Types merged after one-arm `if` reassignment retain the parent/branch union
  instead of becoming opaque, and repeated checks on one `Checker` no longer
  accumulate diagnostics from previous files.

### Pipes, suppressions, and output

- Native-pipe extraction placeholders (R >= 4.3), such as `mtcars |> _$mpg`
  and `df |> _[["col"]]`, resolve to the piped value. Magrittr substitutes every
  `.` occurrence, including nested calls, while `.` and `_` remain specific to
  their respective pipe operators. Invalid cross-operator placeholders now
  report `RY010`. Thanks to [@tjmahr](https://github.com/tjmahr) for reporting
  the placeholder scope issue in [#27](https://github.com/sims1253/ry/issues/27).
- Standalone `# ry: ignore` directives target the next actual code line,
  skipping blank and comment-only lines without mistaking `#` inside strings
  for a directive. Rule lists stop at their closing `]`.
- JSON diagnostics now include the diagnostic `confidence` tier. Severity
  overrides preserve confidence, so `--min-confidence` behaves consistently.

### Editor and language server

- Cached parses are paired atomically with the exact source text they came
  from, preventing concurrent edits from mixing stale text with a newer AST.
- LSP byte offsets and columns now correctly handle UTF-16, non-ASCII
  identifiers, CRLF files, invalid/out-of-range positions, completion and
  signature-help cursors, and code-action edits.
- Rename validates R identifiers (including Unicode and reserved-word rules),
  and loop-variable navigation/rename highlights only the binding identifier.
- Closing a document refreshes cross-file diagnostics in remaining documents.
  Completion and signature help use the embedded base typeshed rather than a
  separate hand-maintained signature table.

### Typeshed and package handling

- Updated the embedded r-typeshed snapshot to `d4453457` (schema 0.0.4),
  including corrected base and rlang metadata, typed rlang missing-value
  constants, and new vctrs function metadata.
- Recursive package scans skip symlinks and unclassifiable directory entries,
  preventing filesystem loops. `.Rbuildignore` handling now distinguishes a
  real trailing `$` anchor from escaped dollar literals.
- R string decoding accepts R's variable-width `\u` (1-4 hex digits) and
  `\U` (1-8 hex digits) forms.

## [0.7.1] - 2026-07-24

### Typeshed semantics

- Updated the embedded r-typeshed snapshot to the schema-2 release-preparation
  commit, including declarative predicate, assertion, return-length, and
  conditional scope-effect metadata.
- The loader accepts both schema 1 and schema 2 and validates semantic metadata
  and standalone-check provenance rather than silently accepting unsupported
  declarations.
- Checker inference now consumes sound metadata for `rlang::is_null`, rlang
  standalone type checks, `intersect()`, `paste()`/`paste0()`, and `source()`.
  Existing contextual rules (flow application, scope ownership, and
  conservative opt-in weakening) remain in the checker.

## [0.7.0] - 2026-07-23

This release follows an audit of 40 R packages and focuses on package-aware
precision, zero-length flow, and high-confidence logic diagnostics. It is a
minor release because it adds rules and intentionally changes project scoping
and diagnostic output.

### Added

- RY099 `discarded-conditional-value` warns when a non-tail, one-arm `if`
  discards a value from a narrowly selected pure expression, catching omitted
  assignments such as `if (z == 0) z + 0.001` without warning on side-effect
  calls or returned branch values.
- RY101 `identical-list-subset-scalar` warns when `identical()` compares a
  single-bracket list subset with an atomic scalar. `x["key"]` remains a list,
  so the comparison is always false and usually needs `x[["key"]]`.
- RY032 recognizes high-confidence vector misuse in `&&`/`||` guards when a
  function independently demonstrates that the guarded parameter accepts
  vectors.
- Compound rejecting guards such as
  `if (!is.numeric(x) || length(x) != 1) stop(...)` establish scalar and type
  facts in their continuation, including longer chains and reversed
  `1 != length(x)` comparisons.

### Package and NSE semantics

- Multi-package CLI invocations are partitioned by enclosing `DESCRIPTION`, so
  functions, bindings, imports, and NSE state no longer leak between package
  roots. Ordinary non-package multi-file scripts remain one project.
- `DESCRIPTION Depends` activates package semantics for projects without a
  `NAMESPACE`, and `NAMESPACE` imports continue to provide exact provenance.
- Magrittr braced right-hand sides bind the `.` pronoun as a unary lambda
  (`x %>% { .$field }`). Data-mask columns correctly shadow same-named base
  functions such as `class`.
- Package scans skip the generated `renv/` bootstrap directory by default.

### Type and flow inference

- `intersect()` length is bounded by its shorter operand, eliminating false
  RY032 findings for scalar-or-empty intersections.
- Zero length propagates through comparisons, `%in%`, and all-empty
  `paste()`/`paste0()` calls; a supplied `collapse` correctly produces a
  scalar string.
- `source()` models its target environment: inside a function it does not open
  the local scope unless `local = TRUE`, while top-level `source()` still
  populates the global scope.
- rlang's `is_null()` narrows like base `is.null()`, removing guarded NULL
  false positives in dplyr and rlang.
- Parameter-default provenance survives flow refinement, while a null-return
  guard alone deliberately does not imply a non-empty vector.

### Fixed

- Impossible standalone type guards are diagnosed without rejecting values
  whose only incompatible evidence comes from an overridable parameter
  default.
- Package-aware dplyr/tidyr and magrittr models no longer require a literal
  `library()` call in package source.
- Ecosystem snapshots were updated after removing guarded rlang RY001/RY070
  false positives.

## [0.6.1] - 2026-07-20

### Added

- Bare calls to rlang's standalone type-check helpers now narrow the checked
  value in subsequent code. The narrowing covers scalar, vector, class, and
  callable checks and accounts for `allow_null` and `allow_na`.
- `stopifnot()` and `assertthat::assert_that()` predicates now narrow values
  after successful assertions.

### Fixed

- Same-named user functions are only treated as rlang standalone checkers when
  their signatures match the expected checker shape, avoiding incorrect
  narrowing for ordinary project functions.
- The scheduled typeshed update workflow now targets `main` and leaves pull
  request creation to maintainers.

### Typeshed

- Updated the vendored r-typeshed snapshot to v0.3.0.

## [0.6.0] - 2026-07-17

Driven by the ry 0.5.0 top-500 CRAN audit (9,237 diagnostics, 1.55%
precision) and a subsequent generalization pass. On the same 504-package
corpus this release emits 3,442 diagnostics (-63%; -69% counting only
warnings/errors), with every previously cataloged true positive either
preserved or its loss individually adjudicated, and ~10 new real shipped
bugs found by the new RY100 rule. Minor bump: scope resolution, rule
routing (RY001/RY003), and quoting semantics intentionally change
reported diagnostics.

### Added

- RY003 `numeric-condition` (Info): numeric `if`/`while` conditions are
  legal, idiomatic R (`if (nchar(x))`, `if (n)`); they are no longer
  RY001 warnings. RY001 keeps the genuinely erroneous modes (character,
  list, NULL, function, length-0).
- RY100 `comparison-inside-math-call` (Warning): a comparison directly
  inside `abs`/`sqrt`/`exp`/`log*`/`floor`/`ceiling`/`round`/`trunc` is
  almost always a parenthesization slip (`abs(x > y)` for `abs(x) > y`).
  Generalizes RY093, ry's highest-precision rule; corpus census found 10+
  real shipped bugs (effects, ggplot2 tests, performance, pracma) at ~100%
  precision after excluding the deliberate `sign(cmp)` indicator idiom.
- RY040 fires on arithmetic with a known-NULL operand (`x / NULL` is
  `numeric(0)`), gated to literal NULLs and missing fields of complete,
  locally built `list(...)` schemas so parameter defaults never trip it.
- Environment profiles: files sourced into a known framework context get
  its ambient bindings. Shiny app trees (`input`/`output`/`session`) ship
  built in; users declare their own via `[[environments]]` in `ry.toml`
  (`name`, `bindings`, `paths`).
- `ry.toml` `max-serialized-bytes` (default 2 MiB) caps `.rda` workspace
  enumeration; oversized workspaces open the file's scope instead of
  stalling the scan (bigD: 190 s -> 0.13 s).
- File collection accepts the full R source extension set (`.S`, `.s`,
  `.q` — boot's entire library was previously invisible), decodes Latin-1
  sources instead of skipping them, and skips `*.Rcheck` build artifacts.

### Scope and name resolution

- `library()`/`require()` of a package without a stub marks the search
  path unknown, silencing RY010 for names that plausibly come from it —
  the single largest false-positive source in the audit (lazy-loaded
  datasets such as `sleepstudy`, `apipop`). Stubbed packages keep full
  checking. `data()`/`load()`/`source()`/`sys.source()` declare the same
  effect via stub metadata; `data(x)` also binds its literal names.
- Attachment is context-scoped to match R's semantics: package `R/` code
  resolves bare names against base plus exactly what NAMESPACE grants
  (`importFrom` names, wholesale `import(pkg)` exports); test and script
  files resolve against the testthat runner world (testthat, the package
  under test, helper/setup and in-file `library()` calls, and DESCRIPTION
  Suggests). Imports no longer leak whole-package exports into files that
  never attached them (arrow's `string`/`int`/`dbl` vs rlang).
- Loop bodies pre-bind names assigned anywhere in the body, so
  loop-carried accumulators read before their first syntactic assignment
  no longer fire RY010.
- `on.exit(expr)` is checked against exit-time bindings (everything the
  function assigns), not walk-order bindings.

### NSE and quoting

- User functions that quote their arguments are detected from their
  bodies — `substitute`/`match.call`/`sys.call`/`bquote` and, via stub
  metadata, the rlang capture family (`enquo`, `enexpr`, `ensym`, plural
  forms, `quos`) — and the property propagates: through direct argument
  forwarding between user functions, from stub eval modes into user
  wrappers, and from S3 methods onto their generics (named method params
  absorbed by the generic's `...` included). lambda.r: 165 -> 0 RY010;
  sparklyr: 93 -> 0.
- Quoted arguments receive no diagnostics at all — they are data, not
  code (igraph's `graph_from_literal(A +-+ B)` no longer type-errors).
- Operands of unknown `%op%` infix operators and unresolvable `.()`
  calls are treated as quoted.
- Formula-interface arguments (`weights`, `subset`, `offset`, `id`,
  `cluster`, `istate`) evaluate inside the `data` mask via the new
  `data_mask_source` stub metadata (stats and survival interfaces).
- String-literal calls (`"paste"(1, 2)`, `"[<-.data.frame"(...)`) resolve
  like identifiers instead of firing RY070; character *variables* in call
  position still do.

### Type system

- Divergence-aware narrowing: a guard whose branch always exits
  (`if (is.null(x)) stop(...)`, `return`, `abort` via the new `no_return`
  stub property, `if (!length(x)) return(...)`) narrows the continuation.
  Never-returning user helpers are detected recursively; a project-local
  function named `abort` is not assumed to diverge.
- Narrowing-installed bindings are tracked explicitly, so a real
  assignment inside a branch always overrides a temporary refinement in
  the post-if merge (fixes stale-NULL cascades through the cross-file
  fixpoint).
- `df[, j]` single-column selection honors `drop = TRUE` (a parser fix:
  the empty row index was previously dropped entirely) and returns the
  column type; scalar subscripts narrow to length 1; negative literals
  keep vector length.
- S3 dispatch walks the full class vector across all method sources;
  `Ops`/`Math`/`Summary` group generics dispatch for data.frames and user
  classes (`df / 2`, `ggplot() + NULL`-style idioms); RY050 fires only
  for generics the project itself demonstrably owns.
- `list(...)` containing dots yields an incomplete schema — a missing
  field is no longer known-NULL; `$`/`[[` through a parameter whose only
  evidence is an overridable NULL default yields unknown.
- A condition typed as a union with at least one valid length-1 logical
  member is not reported (only provably invalid unions are).
- `append()` returns the concatenation of its arguments; `tapply` gained
  a higher-order simplify spec; `mapply` honors `SIMPLIFY = TRUE`
  (all stub-data fixes, vendored from r-typeshed 0.2.0 along with new
  rlang and cli stubs).

### Fixed

- Panic (`index out of bounds`) in quoting-forwarding when a user callee
  and a stub callee had different parameter counts; it crashed scans of
  17 corpus packages (psych, rlang, recipes, …).
- `readLines()` no longer demands `con` (stub had it wrongly required);
  a generator-level fix detects `missing()`-based optionality so the
  whole class (`rlang::env_get(default=)`) cannot recur.
- RY033's stale-type false positives after both `if`/`else` arms rebind a
  variable.
- RY100 subsumes the condition-type diagnostic on the same span (no
  double reporting).

## [0.5.0] - 2026-07-16

Driven by the ranks-301-500 audit (ry 0.4.0 on the top-500 CRAN packages).
Minor bump: RY050's dispatch semantics, RY097's collapse criteria, and the
new binding/quoting/narrowing rules intentionally change reported
diagnostics between versions.

### Performance

- Pipe-chain inference was exponential: each `%>%`/`|>` stage re-inferred
  its entire left-hand side inside the desugared call, so a 20-stage chain
  took ~14 s and longer chains never finished. The inferred LHS type is
  now reused. gt (289 R files, previously unscannable) checks in ~2.4 s.
- The required-parameter force-flow analysis walked each `if` branch twice
  (once for "forces", once for "falls through"), which is exponential on
  long `else if` dispatcher chains. Both facts are now computed in one
  pass. lavaan and stargazer (previously >60 min, never completed) check
  in ~2.3 s and ~0.8 s.

### Fixed

- `assign("name", value, envir = ...)`, `makeActiveBinding()`, and
  `delayedAssign()` with a literal name now create package-level bindings
  (any nesting depth under `R/`). Removes whole-package RY010 cascades in
  clock (204 -> 0), rJava, otel, parallelly, and others. `.packageName`
  is bound in every package namespace.
- A string-literal assignment target (`"Math.foo" <- function(...)`) now
  binds, aliases, and establishes S3 dispatch context (`.Generic`,
  `.Method`) exactly like an identifier target (chron 35 -> 10).
- `alist()` arguments are quoted, never resolved as variables, and the
  call returns a list (Deriv 111 -> 8, ade4 RY010 42 -> 2).
- A union whose members are all functions is callable; RY070 no longer
  fires on `f <- if (p) function(...) ... else function(...)` followed by
  `f(...)`. Argument checks report only findings that hold for every
  member signature. NULL/function unions still report RY070.
- RY097 (not-R-source) now also collapses files that mostly parse as R
  but are riddled with parse errors (>= 5 errors and >= 15% of top-level
  statements): Ratfor, GAUSS, and markdown-table files under `inst/`
  (pacman 270 -> 27, plm 136 -> 36 total).
- `is.list()`/`is.function()`/`is.environment()`/`is.data.frame()` guards
  narrow a parameter whose type came only from its default, so
  `f <- function(x = FALSE) if (is.list(x)) x$field` no longer reports
  RY061 (visNetwork 34 -> 5 RY061).
- Assignments nested inside call arguments of `if`/`while` conditions
  (`if (grepl(p, ti <- text[i]))`) now bind in the enclosing scope
  (litedown 27 -> 10).
- RY033's message no longer claims R compares "byte values"; R coerces
  the numeric operand to character and compares lexicographically.
- The typeshed ships registered-but-unexported base S3 methods (e.g.
  `stats:::print.anova`), so RY050 no longer reports them missing
  (spatial, Cairo). RY050 also honors `<generic>.default` as a valid
  dispatch fallback: `coef(glm_fit)` no longer reports a missing
  `coef.glm`. Consequently RY050 can no longer fire for generics that
  have a `.default` method (such as `print`) — dispatch always succeeds
  for them.

## [0.4.1] - 2026-07-14

### Removed

- `RY095` (negation-comparison-precedence) is retired. The rule assumed C
  operator precedence, but R gives unary `!` lower precedence than
  comparison operators: `!x == y` parses as `!(x == y)`, so every flagged
  site was correct code and the suggested rewrite was a semantic no-op.
  The rule number will not be reused.

### Fixed

- `RY096` no longer fires in functions whose formals include `...`:
  there, `hasArg(name)` legitimately tests for a dots-supplied argument
  (`if (hasArg(b)) list(...)$b`). All 84 corpus hits were this idiom.
  The rule now only flags the provable case — a `hasArg()` naming a
  non-formal in a function without `...` is always `FALSE`.

### Corrections to 0.4.0 release notes

- The scales `!length(x) == 1` guards cited as newly found bugs were not
  bugs; they parse as `length(x) != 1` and behave as intended. The same
  applies to RY095 reports in rpart, mice, quantreg, spdep, and mlflow.

## [0.4.0] - 2026-07-13

Precision release driven by the top-300 CRAN audit: the corpus total fell
from ~23,300 diagnostics to ~6,500 (-72%) while every confirmed real bug
in the audit's regression list still surfaces, and the new rule family
found previously unknown bugs (scales `!length(x) == 1` guards among
them).

### Added

- Typed and required parameter metadata in typeshed signatures, including
  numeric mode unions and strict validation through `ry typeshed validate`.
- R-compatible exact, partial, and positional call-argument matching with
  `RY090` for unknown named arguments, `RY091` for missing required arguments,
  and `RY092` for provable argument type mismatches.

- Runtime custom typeshed loading through the `typeshed` key in `ry.toml` and
  repeatable `--typeshed` flags. Flat and nested stub layouts are supported,
  later directories replace earlier packages, and editor diagnostics use the
  same workspace configuration.
- The embedded typeshed is now a vendored snapshot of the standalone
  `r-typeshed` repository, with schema-version validation and source metadata.
- New mis-parenthesization rule family: `RY093` (comparison inside
  `length()`/`nchar()`/`abs()`, also detected inside `&&`/`||` operands),
  `RY095` (`!x == y` negation-comparison precedence), and `RY096`
  (`hasArg()` naming a non-formal of the enclosing function).
- `RY094`: printf-family (`sprintf`/`gettextf`) literal format strings are
  checked against the supplied argument count.
- `RY097`: files whose top-level statements are mostly unparseable (Ratfor
  sources, broken fixtures) collapse into a single info diagnostic instead
  of hundreds of spurious errors.
- `RY098`: a parameter default referencing a body-local is flagged when an
  execution path can force the default before the local is assigned;
  the idiomatic late-bound default stays silent.
- Confidence tiers: every diagnostic carries `high`/`medium`/`low`
  confidence, output is ranked by tier, diagnostics under `tests/`,
  `data-raw/`, `demo/`, `vignettes/`, and `inst/` are demoted one tier, and
  `--min-confidence` filters both output and exit code. A symbol used in
  value position that only resolves to a function from another namespace is
  reported at high confidence with the resolution target in the message.
- Baseline workflow for incremental adoption: `ry check --write-baseline`
  snapshots current diagnostics (line-number-free matching) and
  `--baseline` / the `baseline` config key subtracts them from later runs.
- Package-aware scan contexts: `tests/testthat/` files see the package's
  own namespace, `testthat`, DESCRIPTION `Depends`/`Suggests`, and
  `helper-*.R`/`setup-*.R` bindings; `data-raw/`, `demo/`, and `vignettes/`
  attach `Depends`; `.Rbuildignore` patterns (Perl regexes) are respected
  without ever excluding `R/` or `tests/`.
- NSE completion: rlang `{{ }}` embrace is recognized as a mask escape
  (typos inside it still flagged), and the `.data$col` / `.data[["col"]]` /
  `.env$var` pronouns resolve against the mask schema or lexical scope.
- Minimum-viable S4 modeling: in-package `setClass`/`setGeneric`/
  `setMethod` are collected across files and dispatched on receiver class,
  `@` slot access is modeled, and vector names survive `t()` and
  `data.frame()` construction.
- Scope and flow fixes: `inherits(x, "cls")` guards narrow types,
  `useDynLib(.fixes=)` prefixes resolve native-routine symbols, R6/S7
  method bodies see `self`/`private`/`super`, top-level
  `assign(..., envir = asNamespace(...))` binds, and replacement-function
  assignments (`dimnames<-` and friends) keep the target bound.
- User-defined infix operators (`%op%`) preserve their operands in the AST;
  zeallot/future `%<-%`/`%->%` destructuring introduces its pattern
  bindings when a package defining the operator is in scope.
- Data-driven semantics via new `injects` stub metadata: `withr::with_*`
  path injection and R6/S7 method-environment bindings now come from the
  typeshed instead of hardcoded checker logic.
- Derived NSE for user-defined functions: a parameter whose first use is a
  defusing call (`enquo`, `enexpr`, `ensym`, `quo`, `substitute`,
  `match.call`, ...) marks call-site arguments as unevaluated, so
  arrow-style test helpers (`compare_dplyr_binding(.input %>% ...)`) stop
  producing unbound-variable noise.
- testthat helper/setup files now propagate their `library()`/`require()`
  attachments (not just bindings) to test files, and the helper filename
  match covers all `helper*`/`setup*` prefixes.
- The data-mask gate is fully data-driven: any loaded package whose stub
  declares `eval` metadata gets NSE treatment (rlist, patrick, bench, ...),
  and user-defined S3 methods inherit the eval metadata of a stubbed
  generic with the same name (dtplyr/dbplyr verb methods).
- `foreach(i = ..., p = ...) %do%/%dopar%/%op% { ... }` binds the loop
  variables in the body regardless of the operator alias used.
- `attach(x)` marks the scope's search path as unanalyzable, silencing
  unbound-variable diagnostics for legacy attach-style scripts.
- Type narrowing applies to expression-position `if` (e.g.
  `x <- if (is.function(f)) f(1) else f`).
- Tidyverse NSE metadata is now GENERATED from installed-package Rd docs
  (`gen_nse_metadata.R` in r-typeshed reads the `<data-masking>` /
  `<tidy-select>` argument markers), giving full dplyr/tidyr coverage and
  a new tidyselect stub; dynamically registered S3 methods inherit their
  generic's NSE metadata.
- `.` binds inside data-masked arguments (dplyr `do()`, pipe idioms), for
  both `%>%` and the native `|>` pipe.
- Defused-parameter derivation covers `{{ }}` embrace usage and exclusive
  `enquos(...)`-style `...` defusal in user functions.
- Inside a data-masked argument with an unknown schema, lexically resolved
  symbols infer as opaque — mask columns may shadow them, so their lexical
  types no longer drive arithmetic/comparison diagnostics.
- Rcpp modeled as a first-class package: `sourceCpp()` carries the new
  `scope_effect: unknown_bindings` stub metadata (compiled exports are
  unknowable), `cppFunction()` returns a function, and `base::attach` now
  uses the same data-driven mechanism instead of a hardcoded recognizer.
- tinytest scan context: files under `inst/tinytest/` see the package's
  own namespace, `tinytest`, and DESCRIPTION Depends/Suggests, mirroring
  the testthat context.

### Changed

- `RY_NO_INSTALLED_LIBRARIES=1` disables resolution of imported-package
  exports from the machine's R installation; the ecosystem regression
  harness sets it so committed snapshots are environment-independent.
- The ecosystem harness report writer is implemented in R (jsonlite)
  instead of python3; the harness now requires `Rscript`.
- Typeshed auditing and stub generation removed from this repository's CI
  and scripts — they live in r-typeshed, whose CI runs them.

## [0.3.0] - 2026-07-11

This release focuses on diagnostic precision, driven by audits of five
real-world CRAN packages (brms, posterior, bayesplot, loo, cmdstanr).
Total diagnostics across those clean corpora dropped from 837 (240
errors) to 30 (1 error — a genuine bug in brms), while all genuine
findings from the audits are still reported. The largest corpus checks
in under a second in release mode.

### Added

- S3 dispatch for operators: binary and unary `Ops` group methods
  (including operator-specific methods such as `+.classname`) defined in
  the checked sources or an attached package are now consulted before
  arithmetic and comparison diagnostics.
- Data-frame schema tracking: `data.frame()` derives column names from
  positional expressions (`data.frame(y, K)` has columns `y` and `K`),
  and column writes via `$`, `[[`, and partial indexed assignment update
  the tracked schema.
- Static dataset inventory: bindings introduced by a package's `data/`
  directory and by `load()` of a project `.rda`/`.RData` file are
  resolved by reading only the top-level tags of the R serialization
  stream (gzip, bzip2, and xz supported) — no R code is executed.
- NSE evaluation modes in typeshed stubs: parameters can be declared as
  data-masked, tidy-select, or quoted, so dplyr-style verbs resolve
  columns instead of flagging them as undefined globals.
- Typeshed stubs for testthat, plus expanded base, Bayesian-stack, and
  dplyr catalogues; stubs can also declare source-relative path
  arguments so `source("helper.R")`-style calls are followed.
- `globals` key in `ry.toml` for names created dynamically by the host
  application or an unresolvable `load()`; only the listed names become
  opaque, without suppressing other diagnostics.
- Lexical closure capture: names assigned anywhere in enclosing function
  bodies are visible inside nested closures, matching R's deferred
  lookup, without making direct read-before-assignment valid.
- Forwarded-default analysis: a formal forwarded into another function
  is credited with the callee's reachable defaults, removing false
  `NULL`-default condition warnings while keeping the genuine ones.

### Fixed

- `importFrom(pkg, name)` now preserves exact binding provenance when a
  stub for the dependency exists, falling back to opaque otherwise.
- Numeric truthiness idioms (e.g. `if (length(x))`) and list/atomic
  equality comparisons no longer produce false diagnostics.
- Various false positives around class-attribute assignment, nested
  record-path writes, S3 predicate narrowing, and dplyr join calls.

## [0.2.0] - 2026-07-10

### Added

- Static resolution of `NAMESPACE` imports, including
  `importFrom(package, name)` and whole-package imports.
- Resolution of exports introduced by `library()` and `require()` without
  executing R or loading package code.
- Support for installed package libraries on Linux, macOS, Windows, and
  renv-managed projects.
- ANSI-colored human-readable diagnostics with
  `--color auto|always|never` and `NO_COLOR` support.
- `RY034` for comparisons with `NA` using `==` or `!=`.
- `RY041` for non-divisible vector recycling.
- `RY042` for arithmetic on factors.

### Fixed

- False-positive `RY010` diagnostics for imported package values such as
  bare `tags` imported from shiny.
- `requireNamespace()` no longer incorrectly introduces unqualified names.
- Package bindings no longer leak between unrelated packages checked together.
- Package-library and R-version precedence now respect the active project,
  including renv libraries.
- Several arithmetic, raw-vector, factor-comparison, assignment, and scope
  inference edge cases.

### Changed

- The minimum supported Rust version is now 1.88 and is verified in CI.
- Human and machine-readable diagnostic output are tested independently;
  JSON and CI formats never contain ANSI escapes.

## [0.1.0] - 2026-07-07

- Initial release.

[Unreleased]: https://github.com/sims1253/ry/compare/v0.11.0...HEAD
[0.11.0]: https://github.com/sims1253/ry/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/sims1253/ry/compare/v0.9.2...v0.10.0
[0.9.2]: https://github.com/sims1253/ry/compare/v0.9.0...v0.9.2
[0.9.0]: https://github.com/sims1253/ry/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/sims1253/ry/compare/v0.7.1...v0.8.0
[0.7.1]: https://github.com/sims1253/ry/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/sims1253/ry/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/sims1253/ry/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/sims1253/ry/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/sims1253/ry/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/sims1253/ry/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/sims1253/ry/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/sims1253/ry/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/sims1253/ry/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/sims1253/ry/releases/tag/v0.1.0
