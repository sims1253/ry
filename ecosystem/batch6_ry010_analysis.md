# Batch 6 RY010 analysis: `arrow/r` and `dbplyr`

Research only (no source edits). Measurements taken on a clean checkout with
two `ry` binaries:

- **baseline** = `HEAD` (commit `af57285`), built in a throwaway git worktree
  at `/tmp/ry-batch6/baseline`.
- **current**  = the uncommitted `dev` working tree (`target/release/ry`,
  built `2026-07-12 21:50`), which includes the in-flight `base.json`
  expansion (+3073 lines) and the `dplyr`/`tidyr` stub additions.

Corpora (shallow clones, exact refs):

- `arrow`  = `apache/arrow` @ `apache-arrow-19.0.1`, checked at `r/R/` (78 `.R` files)
- `dbplyr` = `tidyverse/dbplyr` @ `v2.5.0`, checked at `R/` (100 `.R` files)

> Neither `arrow` nor `dbplyr` is in `ecosystem/packages.txt` yet; "batch 6"
> refers to the parent workflow's grouping. Counts below are the ground truth
> for those two corpora against the two binaries above.

## 1. Measured counts

### Total diagnostics

| package | binary | RY010 | RY061 | RY070 | total |
| :--- | :--- | ---: | ---: | ---: | ---: |
| arrow  | baseline (HEAD) | **3** | 0 | 0 | 3 |
| arrow  | current (dev)   | **0** | 0 | 0 | 0 |
| dbplyr | baseline (HEAD) | **1** | 2 | 1 | 4 |
| dbplyr | current (dev)   | **0** | 2 | 1 | 3 |

**Net RY010 result: every batch-6 RY010 is already resolved by the uncommitted
`base.json` expansion.** `arrow` goes 3 -> 0; `dbplyr` goes 1 -> 0. The 3
remaining `dbplyr` diagnostics are **not RY010** (2 x RY061, 1 x RY070) and are
analysed in section 4.

### Exact baseline RY010 sites (the batch-6 set)

| # | file:line:col | name | category | fix locus |
| :-: | :--- | :--- | :--- | :--- |
| 1 | `arrow/r/R/arrow-package.R:253:20` | `R.version.string` | base value (constant) | base typeshed `globals.ambient` |
| 2 | `arrow/r/R/dplyr-mutate.R:121:41`  | `all.vars` | base function (HOF use) | base typeshed `globals.ambient_functions` |
| 3 | `arrow/r/R/dplyr-summarize.R:50:26`| `all.vars` | base function (HOF use) | base typeshed `globals.ambient_functions` |
| 4 | `dbplyr/R/tbl-lazy.R:72:25`        | `as.name`  | base function (HOF use) | base typeshed `globals.ambient_functions` |

Representative source (identical on both clones):

```r
# arrow-package.R:253  (R.version.string is a base-R top-level character scalar)
grepl("devel", R.version.string)

# dplyr-mutate.R:121 / dplyr-summarize.R:50  (all.vars passed to lapply, not called here)
used_vars <- unlist(lapply(exprs, all.vars), use.names = FALSE)

# tbl-lazy.R:72  (as.name passed to lapply, not called here)
lapply(group_vars(x), as.name)
```

## 2. Grouping by context

The task asked for a breakdown across quoted-expression / data-mask /
test-helper / package-context. For the batch-6 RY010 set the answer is
one-sided:

| context bucket | count | names | notes |
| :--- | ---: | :--- | :--- |
| **base-package-context** (base typeshed gap) | **4** | `R.version.string`, `all.vars` x2, `as.name` | the entire batch-6 RY010 set |
| quoted-expression (`substitute`/`bquote`/`expr`) | 0 | - | none in these corpora |
| data-mask (`{{ }}`, `.data$`, `enquo`) | 0 | - | arrow/dbplyr data-mask usage is already gated by the dplyr NSE stubs |
| test-helper (`testthat`/`local_*`) | 0 | - | `R/` dirs contain no testthat helpers |
| NSE-defuse FFI (`ffi_enquo`, `ffi_*`) | 0 | - | none here (these are an `rlang`-internal pattern, already covered by `rlang`'s own package context) |

**Side-measurement that confirms the package-context mechanism is working.**
If `arrow/r/R` is checked *outside* its package root (so `NAMESPACE` /
`DESCRIPTION` are not walked), baseline ry emits **8** RY010, not 3. The extra
5 names are rlang symbols imported via `importFrom(rlang, ...)` in arrow's
`NAMESPACE`:

`is_call`, `as_function`, `expr`, `quo_is_null`, `as_label`

These 5 are correctly resolved to opaque external bindings *only when* ry can
see the package root, which is exactly the `package-context` fix path. They
are NOT a separate code change - they are the existing NAMESPACE-import
handling doing its job, and they confirm there is no quoted-expression or
data-mask false-positive hiding behind them.

## 3. Exact fix (what the `dev` branch did, and the minimal alternative)

The 4 RY010 are genuine base-R names that were missing from the embedded base
typeshed. Two equivalent fixes exist; the `dev` branch applied the systemic
one.

### 3a. Systemic fix (already on `dev`, preferred)

Extend `crates/ry-typeshed/vendor/base/base.json`. The `Globals` struct has
two relevant allow-lists (see `crates/ry-typeshed/src/lib.rs`):

- `globals.ambient`           - names known to be bound values (suppresses RY010; not callable)
- `globals.ambient_functions` - names known to be functions (suppresses RY010; callable as opaque)

Mapping for the batch-6 set:

| name | section | rationale |
| :--- | :--- | :--- |
| `R.version.string` | `globals.ambient` | base top-level character scalar (a value, not a function) |
| `all.vars`         | `globals.ambient_functions` | base function; used as `lapply(exprs, all.vars)` (HOF, must be callable) |
| `as.name`          | `globals.ambient_functions` | base function; used as `lapply(x, as.name)` (HOF, must be callable) |

Verified present in the current `base.json`:

```
globals.ambient[202]            = "R.version.string"   (list len 262)
globals.ambient_functions[376]  = "all.vars"           (list len 2070)
globals.ambient_functions[494]  = "as.name"
```

(These three entries are part of the wholesale ambient-list expansion that
produced the +3073-line `base.json` diff; they are not special-cased.)

Why `ambient_functions` (not `ambient`) for `all.vars` / `as.name`: both are
passed *by reference* to `lapply`, so they must satisfy `has_function_anywhere`
(see `crates/ry-checker/src/suppress.rs`). `ambient_functions` does;
`ambient` alone would still suppress RY010 but would not count as a function
candidate if the same name were later called directly.

### 3b. Minimal per-project fix (end-user escape hatch, verified)

For a consumer who cannot edit the embedded typeshed, the same three names can
be declared in `ry.toml`, which feeds `Config::globals` -> `external_bindings`
(`crates/ry-cli/src/package_metadata.rs`, `file_bindings.extend(...)`):

```toml
# ry.toml - co-located with (or an ancestor of) the checked tree
globals = ["R.version.string", "all.vars", "as.name"]
```

Verified on the **baseline** binary against `arrow/r/R` copied out of package
context (8 RY010 baseline): adding the three `globals` entries reduces the
count to 5, i.e. exactly those three references are suppressed. (In-package
context the same declaration reduces the real-world 3 to 0.) `external_bindings`
is also a function candidate for `has_function_anywhere`, so HOF use is safe.

## 4. The remaining `dbplyr` noise (non-RY010, unchanged by the RY010 work)

These three diagnostics are present at **both** baseline and current and are
out of RY010 scope, but they are the only remaining `dbplyr` output so they
are recorded here for the parent workflow.

### 4a. RY061 x2 - `utils::data(...)$results` typed as character

```
dbplyr/R/data-lahman.R:77:13       RY061  tables <- utils::data(package = "Lahman")$results[, 3]
dbplyr/R/data-nycflights13.R:53:10 RY061  all <- utils::data(package = "nycflights13")$results[, 3]
```

Root cause: base `data` stub returns `character`

(`crates/ry-typeshed/vendor/base/base.json`, `functions.data.return.mode =
"character"`). With `package =` set, `utils::data` actually returns a *list*
with components `results`, `names`, `call`, `title`; the `$results` subset is
therefore valid and the RY061 ("`$` on atomic character") is a false positive.

Fix category: **stub accuracy** (not RY010 / not a globals entry). Minimal
safe change is to widen `data`'s return to list-like, e.g.

```jsonc
"data": {
  "params": ["..."],
  "return": { "mode": "list", "length": "unknown", "na": false }
}
```

Caveat: `utils::data(...)` *without* `package=` returns a character vector of
dataset names, so a single list return is a slight over-approximation that
could in theory mask a real `$`-on-character bug elsewhere. The two real
call-sites here both pass `package=`, so the list return is the correct model
for dbplyr; the trade-off should be noted in the stub if pursued.

### 4b. RY070 x1 - `f_regex(...)` after an `is_null` guard (null-narrowing)

```
dbplyr/R/translate-sql-string.R:93:7  RY070  f_regex(string, pattern, negate)
```

Source shape:

```r
if (is_null(f_regex)) {                       # line 92
  cli_abort("Only fixed patterns ...", ...)   # line 93 - aborts
} else {
  f_regex(string, pattern, negate)            # line 95 - ry still thinks f_regex is NULL
}
```

`f_regex` is bound to `NULL` (default), so ry types the call site as
call-on-NULL -> RY070. The `else` branch (and, more generally, any code after
a `if (is_null(x)) { <noreturn> }` block) should narrow `f_regex` to non-NULL.

Fix category: **control-flow narrowing** (not RY010 / not a stub). Two parts:

1. Model known aborting functions as `noreturn`. There is no `cli` typeshed
   today (`crates/ry-typeshed/vendor/cli.json` does not exist), so `cli_abort`
   resolves to opaque and ry cannot know it exits. The cheapest entry point is
   the existing "abort/inform" literal list already referenced in
   `crates/ry-checker/src/infer/index.rs` (`"abort" | "inform" ...`): add
   `cli_abort` (and ideally `rlang::abort`, `stop`, `q`) to that noreturn set,
   or ship a minimal `cli` stub marking `cli_abort` noreturn.
2. After the then-branch of an `if (is_null(x)) { ... }` is proven noreturn,
   narrow `x` to non-NULL on the fall-through / else path. The repo already
   has the inverse machinery ("`fix(phase1.2): null-narrowing on is.null
   guards`", per `git log`); extending it to recognise a noreturn then-body is
   the natural place.

This one diagnostic is the single highest-value `dbplyr` improvement after the
RY010 work; it is also the pattern most likely to recur in rlang-heavy code
that wraps `cli_abort` / `abort` guards.

## 5. Tests

The repo's RY010 test convention is a `testdata/*.R` fixture plus an
`assert!(... code != "RY010")` in `crates/ry-checker/src/tests.rs`. The `dev`
branch already added coverage for the `ambient_functions` mechanism:

```rust
// crates/ry-checker/src/tests.rs (already on dev)
#[test]
fn standard_r_inventory_resolves_default_package_symbols() {
    let diags = check(
        "family <- binomial\ndataset <- WWWusage\nhandler <- conditionMessage\nconverter <- as.name\nmaximum <- which.max\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "default-package symbols should exist even without precise signatures: {diags:?}"
    );
}
```

That test already pins `as.name` (batch-6 item 4). To lock in the other two
batch-6 names and the two non-RY010 fixes, the minimal additions would be:

```rust
// Locks all.vars + R.version.string (batch-6 items 1, 2, 3).
#[test]
fn base_ambient_covers_arrow_batch6_names() {
    // all.vars in higher-order position (the exact arrow shape), plus the
    // base scalar R.version.string.
    let src = "vars <- lapply(list(a ~ b), all.vars)\nis_dev <- grepl(\"devel\", R.version.string)\n";
    let diags = check(src);
    assert!(diags.iter().all(|d| d.code != "RY010"),
        "all.vars and R.version.string must resolve via base ambient lists: {diags:?}");
}

// Locks the RY061 utils::data fix once the stub return is widened.
#[test]
fn utils_data_package_arg_returns_list() {
    let diags = check("res <- utils::data(package = \"nycflights13\")$results\n");
    assert!(diags.iter().all(|d| d.code != "RY061"),
        "utils::data(package=...) returns a list; $results is valid: {diags:?}");
}

// Locks the RY070 null-narrowing fix once cli_abort is noreturn.
#[test]
fn null_guard_with_noreturn_then_narrows_else() {
    let src = "f <- NULL\nif (is_null(f)) { cli_abort(\"x\") } else { f(1) }\n";
    let diags = check(src);
    assert!(diags.iter().all(|d| d.code != "RY070"),
        "after a noreturn is_null then-branch, the else path must narrow f to non-NULL: {diags:?}");
}
```

(The first test is safe to add immediately on `dev`; the latter two are
predicated on the section-4 stub/narrowing changes landing first.)

## 6. Summary / takeaways

1. Batch-6 RY010 for `arrow` + `dbplyr` = **4** at HEAD, **0** on the current
   `dev` tree. The uncommitted `base.json` ambient-list expansion fully
   resolves the set.
2. All 4 are the same category - **base-package-context** (missing base
   typeshed entries: `R.version.string`, `all.vars`, `as.name`). Zero
   quoted-expression / data-mask / test-helper / NSE-defuse false positives in
   these two corpora.
3. `arrow`'s 5 rlang imports (`is_call`, `as_function`, `expr`, `quo_is_null`,
   `as_label`) are already handled by ry's `NAMESPACE` `importFrom` mechanism
   (the package-context path) - verified by an out-of-package-context
   re-scan that re-surfaces them.
4. Minimal safe implementation is either (a) the systemic `base.json`
   `globals.ambient` / `globals.ambient_functions` entries already on `dev`,
   or (b) a per-project `ry.toml` `globals = [...]` (verified equivalent on
   the baseline binary).
5. Remaining `dbplyr` noise is non-RY010: 2 x RY061 (fix = widen base
   `data` stub return to `list`) and 1 x RY070 (fix = mark `cli_abort` /
   `stop` noreturn + extend `is_null`-guard narrowing to noreturn then-bodies).

## Artifacts produced

- `/home/m0hawk/Documents/ry/ecosystem/batch6_ry010_analysis.md`  (this file)
- `/tmp/ry-batch6/arrow.json`, `/tmp/ry-batch6/dbplyr.json`        (current-build JSON)
- `/tmp/ry-batch6/arrow.baseline.json`, `/tmp/ry-batch6/dbplyr.baseline.json` (HEAD-build JSON)
- `/tmp/ry-batch6/baseline/target/release/ry`                      (HEAD binary, throwaway worktree)

The git worktree at `/tmp/ry-batch6/baseline` was created with
`git worktree add --detach`; it can be removed with
`git worktree remove /tmp/ry-batch6/baseline` from the repo root when no longer
needed. No tracked source files were modified.
