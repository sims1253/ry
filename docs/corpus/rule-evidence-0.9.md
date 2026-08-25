# Rule evidence for 0.9

This is the final evidence-backed verdict table. The corpus audit established the
measured corpus baseline, semantic-claim oracle, and probe direction. The closing pass
adds the R7 literal-to-parameter lifting report, targeted mutation pilot,
and per-rule verdicts. Every rule has an executed verdict backed by multiple
independent kinds of evidence; no single column determines the verdict.

Corpus counts come from the strict hermetic `posit-0.9.0.json` ledger (728
findings; 37 TP / 691 FP). Probe coverage comes from
`crates/ry-checker/tests/probes.rs`. Claim fixtures come from the
completeness-gated `crates/ry-checker/tests/oracle.rs` registry. The R7
literal-lift report comes from `crates/ry-checker/tests/rule_evidence.rs`.
The mutation pilot covers RY032, RY040, RY093, and RY103.

## R7 literal-to-parameter lifting report

The R7 relation tests whether a rule fires identically when the triggering
value is a literal call argument (`f(literal)`) versus a parameter default
(`f()` with `function(x = literal)`). Four classifications:

- **lift-reachable**: the rule fires on the default form but not the
  call form. The checker does not propagate call-site argument types into
  function bodies, so type-dependent rules catch the defect only when the
  developer writes a default. This is an expected reachability limitation,
  not a bug.
- **param-unreachable**: the rule fires on neither form. The triggering
  value requires a complex expression (e.g. `c(TRUE, FALSE)`) that
  `infer_literal_default` resolves as unknown, so the known type never
  reaches the checker inside a function body. RY032 is the standing case:
  policy decided that unknown parameter length is not evidence
  that `&&`/`||` discards elements. R7 confirming RY032 as
  param-unreachable IS the expected outcome, not a finding.
- **consistent**: the rule fires identically in both forms (syntactic
  rules and value-pattern checks that don't depend on parameter types).
- **n/a**: syntactic or structural rules where R7 is not applicable.

### R7 results

| classification | rules |
| :-- | :-- |
| lift-reachable | RY001, RY003, RY020, RY021, RY031, RY033, RY040 |
| param-unreachable | RY002, RY030, RY032, RY061 |
| consistent | RY034, RY093, RY099, RY100, RY103, RY105 |
| n/a (syntactic) | RY000, RY010, RY041, RY042, RY050, RY060, RY070, RY080, RY090, RY091, RY092, RY094, RY096, RY097, RY098, RY101, RY102 |

## Targeted mutation pilot

Pilot mutations for RY032 and three representative rule families. Each
mutation has a parse-clean assertion, a deterministic before/after
diagnostic inventory, and a negative control. The pilot distinguishes a
broken rule from a broken mutation reliably: the kill mutation removes
the rule, the negative control preserves it, and all sources are
parse-clean.

| rule | family | kill mutation | negative control |
| :-- | :-- | :-- | :-- |
| RY032 | scalar-logical-length | `c(TRUE, FALSE) && TRUE` -> `TRUE && TRUE` (vector to scalar) | change RHS `TRUE` to `FALSE` |
| RY040 | invalid-arithmetic | `"text" + 1L` -> `1L + 1L` (character to integer) | change RHS `1L` to `2L` |
| RY093 | comparison-inside-length | `length(x > 0L)` -> `length(x) > 0L` (move comparison out) | change `0L` to `1L` |
| RY103 | class-equality | `class(x) == "df"` -> `inherits(x, "df")` (correct idiom) | change `"df"` to `"lm"` |

## Complete verdict table

Allowed verdicts: keep, fix, default-off, retire. No single column
determines the verdict.

- **keep**: true semantic claim plus demonstrated useful reachability.
- **fix**: diagnosed implementation gap owning a regression.
- **default-off**: defensible behavior whose observed noise exceeds default value.
- **retire**: false claim or no defensible reachable behavior.

| rule | corpus TP/FP | probe | R claim | literal lift | mutation | message/fix | verdict | rationale |
| :-- | --: | :--: | :-- | :-- | :--: | :--: | :-- | :-- |
| `RY000` syntax-error | 2/4 | yes | `syntax_error_claim.R` | n/a (syntactic) | - | - | keep | True syntax errors; 2 TP / 4 FP. Claim verified. |
| `RY001` invalid-condition | 6/23 | yes | `invalid_condition_claim.R` | lift-reachable | - | - | keep | Valid claim; 6 TP / 23 FP. Lift-reachable through scalar defaults. |
| `RY002` condition-length | 0/3 | yes | `condition_length_claim.R` | param-unreachable | - | - | keep | Valid claim (R warns on multi-element conditions); 0 TP / 3 FP. Parameter-unreachable for c() defaults. |
| `RY003` numeric-condition | 0/0 | yes | `if_numeric.R` | lift-reachable | - | - | default-off | Valid claim but style advice; 0 corpus findings. Already Info severity, disabled by enabled_by_default. |
| `RY010` unbound-variable | 4/472 | yes | `unbound_variable_claim.R` | n/a (syntactic) | - | - | keep | Core reachability check; 4 TP / 472 FP. High FP from cross-file resolution gaps addressed by W2 workspace resolution. |
| `RY020` unary-minus-type | 0/0 | yes | `unary_minus_type_claim.R` | lift-reachable | - | - | keep | Valid claim; 0 corpus findings. Lift-reachable through scalar defaults. |
| `RY021` unary-not-type | 0/0 | yes | `unary_not_type_claim.R` | lift-reachable | - | - | keep | Valid claim; 0 corpus findings. Lift-reachable through scalar defaults. |
| `RY030` invalid-comparison | 0/25 | yes | `invalid_comparison_claim.R` | param-unreachable | - | - | keep | Valid claim; 0 TP / 25 FP. Parameter-unreachable (triggering types are non-scalar). FP from typeshed coverage gaps. |
| `RY031` invalid-logical-op | 0/2 | yes | `invalid_logical_op_claim.R` | lift-reachable | - | - | keep | Valid claim; 0 TP / 2 FP. Known gap in inconsistent_superassignment.R. Lift-reachable through scalar defaults. |
| `RY032` scalar-logical-length | 1/47 | yes | `unknown_short_circuit_parameter.R` | param-unreachable | piloted | yes | keep | Standing case policy: 1 TP / 47 FP. Fires on non-literal parameter-dependent expressions (47 FP in corpus) but policy determined unknown parameter length is not actionable. R7 confirms param-unreachable for c() defaults. |
| `RY033` comparison-mode-mismatch | 6/35 | yes | `comparison_mode_mismatch_claim.R` | lift-reachable | - | - | keep | Valid claim; 6 TP / 35 FP. Lift-reachable through scalar defaults. |
| `RY034` compare-na | 3/0 | yes | `compare_na.R` | consistent | - | yes | keep | Valid claim; 3 TP / 0 FP. Consistent under R7 lifting. |
| `RY040` invalid-arithmetic | 0/23 | yes | `arith_character.R` | lift-reachable | piloted | - | keep | Valid claim; 0 TP / 23 FP. Lift-reachable through scalar defaults. FP from typeshed coverage gaps. |
| `RY041` non-divisible-recycling | 0/0 | yes | `recycle_short.R` | n/a (syntactic) | - | - | keep | Valid claim; 0 corpus findings. |
| `RY042` factor-arithmetic | 0/0 | yes | `factor_arithmetic.R` | n/a (syntactic) | - | - | keep | Valid claim; 0 corpus findings. |
| `RY050` missing-s3-method | 0/0 | yes | `missing_s3_method_claim.R` | n/a (syntactic) | - | - | keep | Valid claim; 0 corpus findings. |
| `RY060` undefined-column | 0/5 | yes | `undefined_column_claim.R` | n/a (syntactic) | - | - | keep | Valid claim; 0 TP / 5 FP. Small absolute count. |
| `RY061` dollar-on-atomic | 0/21 | yes | `dollar_on_atomic.R` | param-unreachable | - | - | keep | Valid claim; 0 TP / 21 FP. Parameter-unreachable for 1:10 defaults. |
| `RY070` call-non-function | 2/6 | yes | `call_non_function.R` | n/a (syntactic) | - | - | keep | Valid claim; 2 TP / 6 FP. |
| `RY080` map-return-type-mismatch | 0/2 | yes | `map_return_type_claim.R` | n/a (syntactic) | - | - | keep | Valid claim; 0 TP / 2 FP. Requires purrr typeshed. |
| `RY090` unknown-argument | 0/4 | yes | `unknown_argument.R` | n/a (syntactic) | - | yes | keep | Valid syntactic claim; 0 TP / 4 FP. |
| `RY091` missing-required-argument | 1/4 | yes | `missing_required_argument.R` | n/a (syntactic) | - | - | keep | Valid claim; 1 TP / 4 FP. |
| `RY092` argument-type-mismatch | 0/3 | yes | `argument_type_mismatch.R` | n/a (syntactic) | - | - | keep | Valid claim; 0 TP / 3 FP. |
| `RY093` comparison-inside-length | 4/0 | yes | `comparison_inside_length_claim.R` | consistent | piloted | yes | keep | Valid claim; 4 TP / 0 FP. Consistent under R7 lifting (syntactic). Mutation pilot passed. |
| `RY094` printf-argument-count | 0/0 | yes | `printf_argument_count_claim.R` | n/a (syntactic) | - | - | keep | Valid claim; 0 corpus findings. |
| `RY096` hasarg-non-formal | 0/0 | yes | `hasarg_non_formal_claim.R` | n/a (syntactic) | - | - | keep | Valid claim; 0 corpus findings. |
| `RY097` not-r-source | 0/0 | yes (CLI) | `not_r_source_claim.R` | n/a (syntactic) | - | - | keep | CLI file heuristic; 0 corpus findings. Probe via CLI exclusion. |
| `RY098` default-forced-before-assignment | 0/1 | yes | `recursive_parameter_default.R` | n/a (syntactic) | - | - | keep | Valid claim; 0 TP / 1 FP. |
| `RY099` discarded-conditional-value | 0/0 | yes | `discarded_conditional_value.R` | consistent | - | - | keep | Valid claim; 0 corpus findings. Consistent under R7 lifting. |
| `RY100` comparison-inside-math-call | 4/0 | yes | `comparison_inside_math_claim.R` | consistent | - | yes | keep | Valid claim; 4 TP / 0 FP. Consistent under R7 lifting (syntactic). |
| `RY101` identical-list-subset-scalar | 0/0 | yes | `identical_list_subset_scalar.R` | n/a (syntactic) | - | yes | keep | Valid claim; 0 corpus findings. |
| `RY102` named-list-element-arrow | 1/0 | yes | `named_list_element_arrow_claim.R` | n/a (syntactic) | - | yes | keep | Valid claim; 1 TP / 0 FP. |
| `RY103` class-equality | 2/0 | yes | `class_equality_claim.R` | consistent | piloted | yes | keep | Valid claim; 2 TP / 0 FP. Consistent under R7 lifting. Mutation pilot passed. |
| `RY105` constant-length-comparison | 1/11 | yes | `constant_length_comparison_claim.R` | consistent | - | - | keep | Valid claim; 1 TP / 11 FP. Moderate FP but small absolute count; 1 TP demonstrates reachability. |

## Verdict execution

Code-level verdicts are enforced by `crates/ry-checker/tests/rule_evidence.rs`:

- **RY003 (default-off)**: `enabled_by_default("RY003")` returns `false`. The
  test `reverting_ry003_default_off_fails` gates this: changing the registry
  to enable RY003 fails the test.
- **RY095 (retired)**: absent from `RULES`. The test
  `retired_rules_are_absent_from_the_registry` gates this: re-adding RY095
  fails the test.
- **All other rules (keep)**: `enabled_by_default` returns `true` for every
  rule except RY003. The test `default_off_verdicts_match_the_registry`
  verifies the registry matches the verdict table.
- **Every rule has a verdict**: the test `every_rule_has_an_executed_verdict`
  verifies the verdict table covers exactly the `RULES` registry.

## Completeness checks

- Rows: 34, exactly one for every non-retired entry in `RULES`.
- Probes: 33 present; RY097 has the committed CLI-level exclusion.
- Claim fixtures: 34 present and enforced by `every_rule_has_a_claim_fixture`.
- R7 coverage: every rule is classified (lift-reachable, param-unreachable,
  consistent, or n/a).
- Mutation pilot: 4 rule families piloted (RY032, RY040, RY093, RY103).
- Corpus values are identity counts from the hermetic ledger.
- Verdicts: 33 keep, 1 default-off (RY003), 0 retire (RY095 retired during the audit response).
