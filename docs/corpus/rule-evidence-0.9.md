# Rule evidence for 0.9 after Plan 34

This is the tracked evidence handoff from Plan 34 to Plan 35. It deliberately
does **not** assign keep, demote, or retire verdicts. Corpus counts come from
the strict hermetic `posit-0.9.0.json` ledger (729 identities; 37 TP / 692 FP),
probe coverage comes from `crates/ry-checker/tests/probes.rs`, and claim fixtures
come from the completeness-gated `crates/ry-checker/tests/oracle.rs` registry.

A “known gap” names an existing oracle fixture whose missing checker behavior
falls directly under that rule. `switch_vector_expr.R` remains an explicit
cross-cutting oracle gap but maps to no current `RULES` entry (there is no
switch-length rule), so it is named here rather than falsely attached to RY002.

| rule | corpus TP | corpus FP | probe present | claim fixture | known gap | P34 note |
| :-- | --: | --: | :--: | :-- | :-- | :-- |
| `RY000` syntax-error | 2 | 4 | yes | `syntax_error_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY001` invalid-condition | 6 | 23 | yes | `invalid_condition_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY002` condition-length | 0 | 3 | yes | `condition_length_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY003` numeric-condition | 0 | 0 | yes | `if_numeric.R` | — | No Posit corpus finding; claim evidence only. No verdict assigned. |
| `RY010` unbound-variable | 4 | 472 | yes | `unbound_variable_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY020` unary-minus-type | 0 | 0 | yes | `unary_minus_type_claim.R` | — | No Posit corpus finding; claim evidence only. No verdict assigned. |
| `RY021` unary-not-type | 0 | 0 | yes | `unary_not_type_claim.R` | — | No Posit corpus finding; claim evidence only. No verdict assigned. |
| `RY030` invalid-comparison | 0 | 25 | yes | `invalid_comparison_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY031` invalid-logical-op | 0 | 2 | yes | `invalid_logical_op_claim.R` | `inconsistent_superassignment.R` | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY032` scalar-logical-length | 1 | 47 | yes | `unknown_short_circuit_parameter.R` | `unguarded_parameter_length.R` | Audited plan discrepancy: P34-W2 removed the dead `Length::Unknown` arm and bare unknown parameters remain quiet, but the separate targeted parameter-vector pattern exists and emitted 47 measured FPs. Carry this evidence into Plan 35’s verdict; do not falsely describe current RY032 as literal-only or manufacture a −47 behavior change. |
| `RY033` comparison-mode-mismatch | 6 | 35 | yes | `comparison_mode_mismatch_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY034` compare-na | 3 | 0 | yes | `compare_na.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY040` invalid-arithmetic | 0 | 23 | yes | `arith_character.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY041` non-divisible-recycling | 0 | 0 | yes | `recycle_short.R` | — | No Posit corpus finding; claim evidence only. No verdict assigned. |
| `RY042` factor-arithmetic | 0 | 0 | yes | `factor_arithmetic.R` | — | No Posit corpus finding; claim evidence only. No verdict assigned. |
| `RY050` missing-s3-method | 0 | 0 | yes | `missing_s3_method_claim.R` | — | No Posit corpus finding; claim evidence only. No verdict assigned. |
| `RY060` undefined-column | 0 | 5 | yes | `undefined_column_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY061` dollar-on-atomic | 0 | 21 | yes | `dollar_on_atomic.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY070` call-non-function | 2 | 6 | yes | `call_non_function.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY080` map-return-type-mismatch | 0 | 2 | yes | `map_return_type_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY090` unknown-argument | 0 | 4 | yes | `unknown_argument.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY091` missing-required-argument | 1 | 4 | yes | `missing_required_argument.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY092` argument-type-mismatch | 0 | 3 | yes | `argument_type_mismatch.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY093` comparison-inside-length | 4 | 0 | yes | `comparison_inside_length_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY094` printf-argument-count | 0 | 0 | yes | `printf_argument_count_claim.R` | — | No Posit corpus finding; claim evidence only. No verdict assigned. |
| `RY096` hasarg-non-formal | 0 | 0 | yes | `hasarg_non_formal_claim.R` | — | No Posit corpus finding; claim evidence only. No verdict assigned. |
| `RY097` not-r-source | 0 | 0 | no — CLI exclusion | `not_r_source_claim.R` | — | The probe harness documents this CLI-level exclusion; the R claim fixture is complete. |
| `RY098` default-forced-before-assignment | 0 | 1 | yes | `recursive_parameter_default.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY099` discarded-conditional-value | 0 | 0 | yes | `discarded_conditional_value.R` | — | No Posit corpus finding; claim evidence only. No verdict assigned. |
| `RY100` comparison-inside-math-call | 4 | 0 | yes | `comparison_inside_math_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY101` identical-list-subset-scalar | 0 | 0 | yes | `identical_list_subset_scalar.R` | — | No Posit corpus finding; claim evidence only. No verdict assigned. |
| `RY102` named-list-element-arrow | 1 | 0 | yes | `named_list_element_arrow_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY103` class-equality | 2 | 0 | yes | `class_equality_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |
| `RY105` constant-length-comparison | 1 | 12 | yes | `constant_length_comparison_claim.R` | — | Measured corpus and claim evidence only. No keep/demote/retire verdict assigned. |

## Completeness checks

- Rows: 34, exactly one for every non-retired entry in `RULES`.
- Probes: 33 present; RY097 has the committed CLI-level exclusion.
- Claim fixtures: 34 present and enforced by `every_rule_has_a_claim_fixture`.
- Registered claim fixtures are never `known-gap`; the named gaps remain
  separate, explicit evidence.
- Corpus values are identity counts, not inferred from rule semantics.
