# Local checker experiment report

Use `scripts/experiment_report.py` when a checker change needs both a diagnostic
control and a performance comparison. The command reads a JSON manifest. It
resolves each Git ref to a commit, creates two detached worktrees in a new output
directory, and writes `report.json`, `report.md`, and raw command output there.
It does not reset your checkout or change an oracle, corpus ledger, or baseline.

Create a manifest like this:

```json
{
  "schema_version": 1,
  "hypothesis": "The candidate preserves ifelse mode diagnostics",
  "expected_change": "No diagnostic change for these controls",
  "invariants": ["No reviewed true positive disappears", "The quiet control stays quiet"],
  "reference": "HEAD~1",
  "candidate": "HEAD",
  "fixtures": [
    {
      "name": "ifelse_witness",
      "role": "witness",
      "path": "crates/ry-checker/testdata/err_ifelse_mode_collapse.R",
      "expected_additions": [],
      "expected_removals": []
    },
    {
      "name": "ifelse_quiet",
      "role": "quiet_control",
      "path": "crates/ry-checker/testdata/ok_ifelse_mode_collapse.R"
    }
  ],
  "targeted_checks": [
    {
      "name": "ifelse_oracle",
      "argv": ["cargo", "test", "-p", "ry-checker", "--test", "oracle", "claim_fixtures_demonstrate_their_r_premise", "--", "--include-ignored"],
      "requires_r": true
    }
  ],
  "r_packages": ["rlang"],
  "instructions": {"enabled": true, "backend": "auto", "budget": 180, "repetitions": 3}
}
```

Choose a witness that should expose the change and a nearby quiet control.
Set `expected_additions` and `expected_removals` to exact `code`, `line`, and
`column` identities when a finding change is intended. The utility compares
each finding, even when the total count stays the same. For a pinned corpus
finding, set `path` to the package-relative file, such as `R/serve.R`. Add
`"workload": {"repository": "/absolute/local/blogdown-checkout",
"revision": "07f3de89f672c0155149fd657a0946c29684a7f8"}` and
`"triage": {"ledger": "docs/corpus/posit-0.9.0.json", "package": "blogdown",
"path": "R/serve.R"}`. The repository must already contain that commit. The
utility checks out the pin in its output directory and verifies it against the
ledger before using the ledger's `true_positive` or `false_positive` label. It
does not fetch a package from the network. A removed reviewed
true positive fails the fixture stage, even if you listed it as an expected
removal. The test suite includes a modeled version of the #324 failure shape;
it does not claim to rerun that historical change.

Run a targeted investigation:

```sh
python3 scripts/experiment_report.py experiment.json --profile targeted --output /tmp/ry-experiment-001
```

The target directory must be new and outside the checkout. The targeted
profile runs the manifest's checks, builds a checker from each commit, checks
the selected fixtures, and runs instruction measurements when enabled.
Targeted status `passed` never sets `fully_validated` to true. Set
`"instructions": {"enabled": false}` for a quick diagnostic investigation;
the report records that stage as `skipped`.

Run a full validation with `--profile full` and a new output directory. It
runs `cargo test --workspace`, Clippy with warnings denied, formatting, and
the full R oracle in that order before the targeted checks. It then checks
fixtures and runs the existing `ecosystem/instructions.py` harness on both
commits. The same harness and pinned package sample are used on both sides.
The report records the producing commits, Git trees, binary hashes, Cargo
lockfiles, toolchain and selected R package versions, fixed thread settings,
sample pins, exact command arrays, and raw output. It records only the fixed
environment variables it sets. It never records your full environment.

`fully_validated` is true only after the full profile passes every required
gate, every selected control, and a comparable instruction measurement. A
missing R installation makes its check `unavailable`. A failed command is
`failed`; an interrupted run is `cancelled`; changed workload pins or
measurement metadata are `incomparable`. A stage that was not selected or
could not run after an earlier failure is `skipped`. None of these states is a
passing full validation. Instruction ledgers keep all measured samples; count
changes still have uncertainty from the host and randomized hashing. The
report does not combine correctness and performance into one score.

The utility does not upload reports. Review the raw output and the existing
corpus triage before sharing or changing any expectation.
