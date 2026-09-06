# Corpus instruction counts

The fixed sample in [instruction-packages.txt](../../ecosystem/instruction-packages.txt)
tracks the cost of checking glue, stringr, and withr. These are full commit pins
from the ecosystem corpus. The harness checks each package's `R/` directory;
it does not install packages or execute R code.

Run this command from the repository root on Linux:

```sh
python3 ecosystem/instructions.py measure --output instruction-current.json
```

The command builds ry in release mode with `--locked`, fetches the pinned
sources, and records three counts per package. It uses `perf stat` with the
user-space `instructions:u` event when available. If perf is missing, denied
by the host, or cannot count instructions, it uses Valgrind's Callgrind `Ir`
counter. Install `perf` or `valgrind`, plus the usual Rust build dependencies.
Use `--backend perf` or `--backend callgrind` to require one counter.

Each count includes process startup, analysis, and JSON formatting. Output
goes to `/dev/null`. The recorded value is the median; the ledger retains all
three measurements. This measures instructions, not elapsed time. The timeout
is only a resource limit. Rust's randomized hashing can still cause small
instruction-count differences, so this is not a claim of perfectly repeatable
counts.

The harness sets `RAYON_NUM_THREADS=1` and `RY_NO_INSTALLED_LIBRARIES=1`, and
places an empty `R/ry.toml` in each private sample checkout. This excludes host
R libraries and project configuration. Counter processes inherit only `PATH`
plus the fixed variables and an empty temporary `HOME`; Callgrind ignores
user option files. The sample commit and Git tree identify
the source bytes; a dirty sample checkout is rejected. The ledger also records
the binary hash, ry revision, compiler, build flags, architecture, CPU, libc,
counter version, harness hash, and measurement settings.

## Compare runs

```sh
python3 ecosystem/instructions.py compare \
  docs/corpus/instructions-baseline.json instruction-current.json
```

The report warns when a package grows by more than 10%. Use `--threshold 15`
to choose a different percentage. Growth never changes the comparison's exit
status. Missing measurements are `UNAVAILABLE`; changed sample pins or
measurement metadata are `INCOMPARABLE`. Perf and Callgrind counts are never
compared with each other. A failed measurement command exits nonzero and
preserves completed rows when the failure occurs during package measurement.
An unavailable counter or failed build produces a ledger with unavailable rows.
The perf parser follows the documented [CSV field order](https://man7.org/linux/man-pages/man1/perf-stat.1.html#CSV_FORMAT)
and rejects missing or multiplexed counters.

The default measurement budget is 180 seconds, with at most 60 seconds for
each counter invocation. The build has a separate 600-second limit. Use
`--budget` and `--package-timeout` for larger local runs. Fetching package
sources has a separate per-command timeout of 120 seconds. Keep the fixed
sample small enough to fit the CI budget.

## CI and baseline updates

[Corpus instruction counts](../../.github/workflows/instructions.yml) runs on
merges to main, daily, and on manual dispatch. Changes to the harness also run
it in pull requests. The job has a 20-minute limit and is warn-only. The normal
single-file performance tests remain the blocking latency checks.

The committed [baseline ledger](instructions-baseline.json) contains actual
Callgrind measurements. Its host may differ from a CI runner. CI therefore
checks out the baseline's fixed `source_revision` and measures it again with
the same compiler, counter, and sample as the current code. Both builds use
separate target directories. The job summary reports these comparable deltas
and explains whether the historical absolute counts are comparable. Artifacts
retain both ledgers and the report for 90 days.

The reference revision stays fixed between runs; it does not follow main.
Change it only in a reviewed baseline update after investigating the reported
growth. Generate the replacement ledger from a clean checkout of the intended
reference revision, inspect the counts, and commit it separately from a
performance change. Keep the package pins unchanged so a new baseline does
not hide a change in the workload. A toolchain update should also review the
new noise floor before changing the warning threshold.
