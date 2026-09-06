# Scope journal prototype

This branch preserves an experiment for issue #130. It is not proposed for
production: the final version still adds about 27% instructions on dense branches
that alternate inferred types, while the ordinary corpus changes by less than 1%.

`results.json` records the final source commit, input hashes, tool versions and
measurements. `profile.py` builds the locked standalone harness, generates the
fixtures, profiles both strategies, and checks that their diagnostics match.
Use a new output directory:

```sh
python3 experiments/scope-journal/profile.py \
  --out /tmp/ry-scope-profile --tools callgrind dhat
```

The experiment applies only to statement `if` branches. `RY_SCOPE_JOURNAL=0`
uses the existing clone path; `1` uses journal snapshots. No production branch
should inherit this environment switch.
