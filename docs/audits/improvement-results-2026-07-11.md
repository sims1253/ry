# Improvement-plan verification — 2026-07-11

Release build (`cargo build --release -p ry-cli`) checked the same five clean
corpora used by `improvement-plan-2026-07-10.md`:

| Corpus | Diagnostics | Errors | Warnings | Wall time |
| --- | ---: | ---: | ---: | ---: |
| brms | 18 | 1 | 17 | 0.75s |
| posterior | 1 | 0 | 1 | 0.16s |
| bayesplot | 8 | 0 | 8 | 0.23s |
| loo | 2 | 0 | 2 | 0.10s |
| cmdstanr | 1 | 0 | 1 | 0.13s |
| **Total** | **30** | **1** | **29** | **1.37s** |

The command for each checkout was:

```sh
/home/m0hawk/Documents/ry/target/release/ry check . \
  --output-format json --exit-zero
```

All three positive controls remain:

- brms `R/formula-ac.R:693`, RY001 warning;
- brms `tests/testthat/tests.stancode.R:2197`, RY060 error;
- posterior `R/uniformity_test.R:188`, RY001 warning.

Without `--exit-zero`, brms exits 1 for its single error; posterior, bayesplot,
loo, and cmdstanr all exit 0.

The pre-change release baseline was 0.66s / 0.25s / 0.26s / 0.17s / 0.20s
for brms / posterior / bayesplot / loo / cmdstanr respectively. Intermediate
tranche timings were not captured; the final largest-corpus time remains below
the plan's one-second budget.

The workspace gate passed with 457 tests, Clippy with warnings denied, the R
oracle suite, and `cargo fmt --all -- --check`.
