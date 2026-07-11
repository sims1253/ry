# `brms` corpus audit (2026-07-10)

## Scope and reproduction

This audit catalogs `ry check .` against a clean `brms` checkout without
changing `brms`.

- `ry`: `ab0ef50` (`v0.2.0`)
- `brms`: `e71e9d74`
- R: 4.6.1
- command, run from the `brms` root:

  ```sh
  ../ry/target/debug/ry check . --output-format json --exit-zero
  ```

The command was run twice (once as JSON and once with `--statistics`) and
produced the same counts. It checked 128 files in about 2 seconds. The JSON
form is the reproducible full-fidelity catalogue; the groupings below are
derived from it with `jq`.

## Headline result

`ry` emitted 566 diagnostics: 149 errors and 417 warnings across 52 files.
Manual triage found two likely `brms` report candidates, 148 false-positive
errors, 397 false-positive warnings, and 19 warnings that accurately describe
intentional numeric-truthiness idioms but are not useful findings for `brms`.

| Location | Errors | Warnings | Total |
| --- | ---: | ---: | ---: |
| `R/` | 102 | 55 | 157 |
| `doc/` | 4 | 31 | 35 |
| `tests/` | 43 | 331 | 374 |
| **Total** | **149** | **417** | **566** |

| Rule | `R/` | `doc/` | `tests/` | Total | Triage |
| --- | ---: | ---: | ---: | ---: | --- |
| RY001 invalid condition | 23 | 0 | 0 | 23 | 1 report candidate, 3 false positives, 19 intentional idioms |
| RY010 unbound variable | 32 | 31 | 331 | 394 | false-positive clusters |
| RY030 invalid comparison | 3 | 0 | 0 | 3 | false positives |
| RY040 invalid arithmetic | 13 | 0 | 3 | 16 | false positives |
| RY060 undefined column | 76 | 4 | 1 | 81 | 1 report candidate, 80 false positives |
| RY061 dollar on atomic | 3 | 0 | 1 | 4 | false positives |
| RY070 call non-function | 7 | 0 | 38 | 45 | false positives |

The useful signal is therefore two diagnostics out of 566 (0.35%). More
importantly, only one of the 149 error-severity diagnostics is likely real.

## `expect_error()` and test code

None of the 149 error-severity diagnostics is an error intentionally exercised
inside `testthat::expect_error()`. The 43 errors under `tests/` comprise:

- 38 valid calls to a function whose name is also bound to a non-function
  (`nobs()` 36 times and `data()` twice);
- 3 uses of the S3 `+.stanvars` operator;
- 1 imprecise S3-generic return type at `coef(fit_mv)$fosternest`; and
- 1 likely real test setup defect at `tests/testthat/tests.stancode.R:2197`.

`brms` does contain many `expect_error()` calls, and some warning-level RY010
findings occur in NSE expressions used by tests of invalid input. That is a
separate NSE-modeling problem, not evidence that the checker should ignore a
whole expected-error expression.

Blanket suppression inside `expect_error()` is not recommended. It would hide
unrelated setup defects and would have made the real test candidate in this
audit easier to miss. If test-framework awareness is added later, it should
only suppress or annotate a diagnostic when `ry` can associate it with the
specific error contract being asserted.

## Likely `brms` report candidates

No changes should be made to `brms` as part of this work. These are candidates
to report upstream after a maintainer reviews the evidence.

### 1. Positive-definite check accepts an indefinite matrix

- Diagnostic: `R/formula-ac.R:693`, RY001
- Code:

  ```r
  if (min(eigen(M)$values <= 0)) {
    stop2("'M' for FCOR terms must be positive definite.")
  }
  ```

The comparison occurs before `min()`. Consequently the condition is true only
when every eigenvalue is non-positive. A matrix with eigenvalues `1` and `-1`
is indefinite but the current expression evaluates to false:

```text
M eigenvalues                    current expression    intended expression
1, -1                            0                     TRUE
1,  2                            0                     FALSE
-1, -2                           1                     TRUE
```

The direct R probe used for the table was:

```r
min(eigen(M)$values <= 0)       # current
min(eigen(M)$values) <= 0       # intended predicate
```

There is no focused test of `validate_fcor_matrix()` rejecting an indefinite
matrix in the current `brms` test suite.

### 2. Overimputation test adds NAs to the wrong column

- Diagnostic: `tests/testthat/tests.stancode.R:2197`, RY060
- Code:

  ```r
  dat = data.frame(y = rnorm(10), x_x = rnorm(10), g = 1:10, z = 1)
  dat$x[c(1, 3, 9)] <- NA
  bform <- bf(y ~ mi(x_x)*g) + bf(x_x | mi(g) ~ 1) + set_rescor(FALSE)
  ```

R's `$<-` assignment creates a new `x` column; it does not partially match the
existing `x_x` column. The modeled `x_x` remains complete:

```text
x_x_na=0 x_na=3 names=x_x,x
```

The test is named “Stan code for overimputation” but this setup does not put
missing values in the modeled `x_x` variable. The generated-code assertions
may still pass for reasons independent of actual missingness, so this is a
test-quality issue rather than evidence of a user-visible runtime failure.

## False-positive catalogue

### Error-severity diagnostics (148 of 149)

| Cause | Rules | Count | Representative behavior |
| --- | --- | ---: | --- |
| Data-frame schema is not updated through chained, nested, vectorized, or dynamic column assignments | RY060 | 72 | `out$vars <- out$byvars <- out$covars <- ...`, `out[cnames] <- ...`, `out[[col]] <- ...` |
| Data-frame constructor/verb schemas lose inferred names or newly created columns | RY060 | 8 | `data.frame(xname, uni_me)`, `transform(..., grainsize = ...)` |
| R function-position lookup does not treat imported/external/known-export bindings as possible functions | RY070 | 45 | local `ndraws = NULL` with imported `ndraws()`, scalar `nobs` with `nobs()` |
| S3 arithmetic dispatch is not applied | RY040 | 16 | `brmsprior + brmsprior` and `stanvars + stanvars` |
| List comparison model is too strict or indexing lost the element type | RY030 | 3 | `list("NA") == "NA"` is valid R; matrix/data-frame `[,]` was inferred as list |
| Guard refinement or S3-generic return type is missing | RY061 | 4 | `if (is.brmsfit(fit)) x <- fit; x$criteria <- ...`, `coef(fit_mv)$fosternest` |
| **Total** |  | **148** |  |

The RY060 split deliberately excludes the `dat$x` report candidate. Of its 80
false positives, 72 are assignment-flow failures and 8 are constructor/NSE
schema failures.

The RY070 cases are valid R because function calls perform function-position
lookup. A non-function binding is skipped while R searches enclosing
environments for a function. `ry` already implements this for functions in its
typeshed and function table, but `has_function_anywhere()` does not consult the
per-file `external_bindings` populated from `NAMESPACE` or a complete base
export set. This explains the 45 cases in this corpus:

- `ndraws()` (4), imported from `posterior`;
- `theme()` (1), from the whole-package `ggplot2` import;
- `plot()` and `as.array()` (2), base/recommended-package functions;
- `nobs()` (36), while the test file also binds `nobs <- 40`; and
- `data()` (2), while the test scope also has a `data` list.

### Warning-severity diagnostics

RY010 accounts for 394 warnings and partitions cleanly into these missing
models:

| Cause | Count | Examples |
| --- | ---: | --- |
| Package datasets and `data()` bindings | 178 | `epilepsy`, `inhaler`, `kidney`, `BTdata`, `nhanes` |
| NSE/data-mask symbols | 203 | `prior(..., class = Intercept)`, `with(prior, order(resp, ...))`, `transform()` expressions |
| Missing base bindings used as values/callbacks | 13 | `attr`, `structure`, `summary`, `mapply`, `apply`, `.BaseNamespaceEnv` |
| **Total** | **394** |  |

The dataset count consists of 161 test findings and 17 documentation findings.
The current source package has `LazyData: true` and ships `epilepsy`, `inhaler`,
`kidney`, and `loss`; tests attach `brms`. External datasets are also introduced
explicitly with calls such as `data("BTdata", package = "MCMCglmm")`.

The 23 RY001 warnings split as follows:

- 1 useful finding: the positive-definite predicate above;
- 3 false positives where scalar indexing was inferred as `logical<len=0>`;
- 19 accurate but low-value warnings on intentional count/truthiness idioms,
  mostly `if (sum(predicate))`, `if (NROW(x))`, and `if (NCOL(x))`.

Suppressing all reducer-based numeric conditions would lose the useful FCOR
finding. Narrow recognition of established count idioms is safer.

## Affected files

| File | Errors | Warnings | Total | Rules |
| --- | ---: | ---: | ---: | --- |
| `R/backends.R` | 0 | 2 | 2 | RY001 |
| `R/brm.R` | 3 | 0 | 3 | RY061 |
| `R/brmsfit-helpers.R` | 1 | 0 | 1 | RY070 |
| `R/brmsformula.R` | 0 | 1 | 1 | RY010 |
| `R/brmsframe.R` | 0 | 1 | 1 | RY010 |
| `R/conditional_effects.R` | 1 | 1 | 2 | RY001, RY030 |
| `R/data-helpers.R` | 0 | 1 | 1 | RY001 |
| `R/data-predictor.R` | 0 | 2 | 2 | RY001, RY010 |
| `R/distributions.R` | 0 | 8 | 8 | RY001 |
| `R/families.R` | 0 | 1 | 1 | RY010 |
| `R/formula-ac.R` | 28 | 1 | 29 | RY001, RY060 |
| `R/formula-ad.R` | 1 | 0 | 1 | RY030 |
| `R/formula-gp.R` | 10 | 0 | 10 | RY060 |
| `R/formula-re.R` | 0 | 1 | 1 | RY001 |
| `R/formula-sm.R` | 4 | 0 | 4 | RY060 |
| `R/formula-sp.R` | 30 | 6 | 36 | RY001, RY010, RY060 |
| `R/hypothesis.R` | 1 | 0 | 1 | RY070 |
| `R/loo.R` | 0 | 1 | 1 | RY010 |
| `R/lsp.R` | 0 | 1 | 1 | RY010 |
| `R/misc.R` | 0 | 1 | 1 | RY010 |
| `R/model_weights.R` | 3 | 0 | 3 | RY070 |
| `R/plot.R` | 1 | 0 | 1 | RY070 |
| `R/posterior_epred.R` | 0 | 7 | 7 | RY010 |
| `R/posterior_predict.R` | 0 | 4 | 4 | RY010 |
| `R/posterior_samples.R` | 1 | 0 | 1 | RY070 |
| `R/predictor.R` | 0 | 3 | 3 | RY001, RY010 |
| `R/prepare_predictions.R` | 0 | 3 | 3 | RY001, RY010 |
| `R/prior_draws.R` | 0 | 2 | 2 | RY001, RY010 |
| `R/priors.R` | 13 | 6 | 19 | RY010, RY040 |
| `R/restructure.R` | 0 | 1 | 1 | RY001 |
| `R/stan-prior.R` | 4 | 0 | 4 | RY060 |
| `R/summary.R` | 1 | 1 | 2 | RY010, RY030 |
| `doc/brms_customfamilies.R` | 0 | 3 | 3 | RY010 |
| `doc/brms_missings.R` | 0 | 5 | 5 | RY010 |
| `doc/brms_monotonic.R` | 3 | 0 | 3 | RY060 |
| `doc/brms_multivariate.R` | 0 | 4 | 4 | RY010 |
| `doc/brms_nonlinear.R` | 0 | 5 | 5 | RY010 |
| `doc/brms_threading.R` | 1 | 14 | 15 | RY010, RY060 |
| `tests/brmsfit_examples.R` | 0 | 9 | 9 | RY010 |
| `tests/local/tests.models-1.R` | 0 | 19 | 19 | RY010 |
| `tests/local/tests.models-2.R` | 3 | 13 | 16 | RY010, RY061, RY070 |
| `tests/local/tests.models-3.R` | 0 | 16 | 16 | RY010 |
| `tests/local/tests.models-4.R` | 0 | 10 | 10 | RY010 |
| `tests/local/tests.models-5.R` | 0 | 16 | 16 | RY010 |
| `tests/testthat/tests.brm.R` | 0 | 1 | 1 | RY010 |
| `tests/testthat/tests.brmsfit-helpers.R` | 0 | 1 | 1 | RY010 |
| `tests/testthat/tests.brmsfit-methods.R` | 36 | 1 | 37 | RY010, RY070 |
| `tests/testthat/tests.families.R` | 0 | 5 | 5 | RY010 |
| `tests/testthat/tests.priors.R` | 0 | 5 | 5 | RY010 |
| `tests/testthat/tests.stan_functions.R` | 0 | 16 | 16 | RY010 |
| `tests/testthat/tests.stancode.R` | 3 | 181 | 184 | RY010, RY040, RY060 |
| `tests/testthat/tests.standata.R` | 1 | 38 | 39 | RY010, RY040 |

## Proposed `ry` work plan

Each item should begin with a minimal exact-diagnostic fixture and retain the
two useful findings as red-capable corpus checks.

### P0: recover error-level precision

1. **Propagate data-frame schema mutations recursively.** Teach assignment in
   statement and expression position to update an indexed root (`d$b[i] <-`,
   `d$b <- d$c <-`, `d[[name]] <-`). Evaluate statically known character
   vectors and simple loop iterators for `d[names] <-`/`d[[name]] <-`. Preserve
   RY060 for a genuinely new read/partial-name mistake such as `dat$x` when
   only `x_x` exists. Expected impact: remove 80 false errors while keeping the
   one test candidate.

2. **Use external and known-export bindings in function-position lookup.**
   Make `has_function_anywhere()` treat a matching `external_bindings`, known
   base export, or discovered S3 generic name as a possible function, then
   resolve conservatively to opaque when no typed signature exists. Add
   fixtures for `importFrom(posterior, ndraws)`, a whole-package import, a base
   generic, and a local non-function binding with the same name. Expected
   impact: remove all 45 RY070 false errors.

3. **Dispatch S3 `Ops`/operator methods before scalar arithmetic errors.**
   Recognize operator method definitions whose first argument follows the
   `e1`/`e2` convention, and consult classed methods for `+` before emitting
   RY040. Expected impact: remove all 16 RY040 false errors.

4. **Correct comparison/index element typing.** R permits equality comparison
   between a list of atomic elements and an atomic vector, while list-to-list
   comparison remains invalid. Preserve column/element types through `[,]`.
   Expected impact: remove all 3 RY030 false errors.

5. **Refine guarded values and opaque S3 generic results.** Narrow `fit` after
   predicates such as `is.brmsfit(fit)`, and avoid treating an unknown S3
   method result from `coef(classed_value)` as a proven atomic vector.
   Expected impact: remove all 4 RY061 false errors.

Acceptance criterion for P0: the `brms` corpus has one error, the intentional
`dat$x`/`x_x` mismatch, rather than 149. Do not achieve this by excluding tests
or demoting error rules globally.

### P1: recover warning-level precision

6. **Model package datasets and `data()`.** For a source package, discover
   dataset names from `data/` without executing R and expose them when that
   package is attached. Treat `data(name, package = ...)` as NSE and introduce
   the named binding for subsequent statements. Installed-package metadata can
   be consulted when available, with opaque fallback. Expected impact: remove
   178 RY010 warnings.

7. **Complete base/recommended-package binding coverage.** A base export may be
   a valid value/callback even when `ry` lacks its return signature. Seed known
   exports as opaque bindings separately from typed signatures. Expected
   impact: remove 13 RY010 warnings without pretending to know their types.

8. **Represent NSE at parameter granularity.** Extend function metadata or
   call models so selected parameters can be marked as quoted symbols/data-mask
   expressions. Apply this to `brms` prior/formula APIs and improve schema
   propagation through base `with`, `transform`, `subset`, and `merge`. Avoid
   adding whole functions to `is_nse_symbol_fn`, because that suppresses real
   diagnostics in ordinary arguments. Expected impact: remove 203 RY010
   warnings.

9. **Fix scalar-index length inference.** `x[i]` and matrix/data-frame element
   access with scalar indices should normally produce length one, not zero.
   Expected impact: remove 3 RY001 false positives.

### P2: tune intentional-condition noise without losing signal

10. Extend numeric-truthiness recognition narrowly to `NROW()`/`NCOL()` and
    well-typed `sum(logical)` count checks. Keep suspicious constructions such
    as `min(predicate)` visible; this audit demonstrates that they can reveal a
    real precedence bug. Expected impact: up to 19 fewer low-value warnings.

### Regression strategy

- Add minimal checker fixtures for each semantic cause, not one fixture per
  `brms` line.
- Add a small `brms`-derived corpus snapshot containing both report candidates
  and representative false positives.
- Assert exact diagnostic multisets so fixes cannot silently trade false
  positives for false negatives.
- Keep `expect_error()` handling out of the first implementation tranche; it
  did not cause the error counts in this audit.
- Re-run the command at the top of this document after every tranche and update
  the count table in the PR description.
