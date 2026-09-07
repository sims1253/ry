# Expression-if scope allocation

Expression-form `if` arms now use one child scope at a time. The checker drops
that child after retaining its type and effect flags. An absent `else` returns
NULL without cloning a scope. Both explicit arms still see the same parent;
effect flags propagate only after both arms finish.

This removes an unused narrowing-name set and the absent-else clone. It also
reduces peak live heap for nested expressions with two explicit arms. Statement
branches and the public `Scope` API are unchanged. Snapshots still cost O(scope
size), so [#130](https://github.com/sims1253/ry/issues/130) remains open.

## Measurement

Two isolated release binaries compare baseline `d5414ff` with the scope change
from `fab4952`. The measurements exclude the separate, equivalent
walker-condition cleanup required by the local Clippy gate.
DHAT 3.18.1 measures the complete CLI process, including parsing and output.
Rust is 1.96.1 on Linux x86-64. Both runs set `RAYON_NUM_THREADS=1` and
`RY_NO_INSTALLED_LIBRARIES=1`. The synthetic files match the 1,024-binding
Criterion inputs: 24 nested expression-form conditionals, with either one arm
or two arms. The corpus command checks 461 files. Diagnostics match byte for
byte for all three workloads.

| Workload | Baseline allocated bytes | Changed allocated bytes | Change |
| :-- | --: | --: | --: |
| No else | 68,204,571 | 45,344,963 | -33.52% |
| Explicit else | 68,180,087 | 68,180,095 | unchanged |
| Fixture corpus | 167,049,810 | 166,988,530 | -0.037% |

| Workload | Baseline peak live bytes | Changed peak live bytes | Change |
| :-- | --: | --: | --: |
| No else | 23,879,204 | 16,258,540 | -31.91% |
| Explicit else | 23,886,132 | 16,266,252 | -31.90% |
| Fixture corpus | 30,425,122 | 30,425,122 | unchanged |

Allocated bytes are cumulative allocations. Peak live bytes are live heap,
not process RSS. The eight-byte explicit-else difference is negligible; binary
paths differ between runs. These results do not establish a wall-time speedup.
The ordinary corpus benefit is small. Exact totals, input hashes, binary hashes,
and output hashes are in [the measurement record](if-expression-scopes.json).

## Reproduce

Create two worktrees at `d5414ff`. Apply commit `fab4952` to one
worktree with `git cherry-pick`, leaving out the separate walker cleanup. Build
both with `cargo build --release --locked -p ry-cli`, and save each binary. Generate the
two synthetic inputs in a separate output directory:

```python
from pathlib import Path

for with_else in [False, True]:
    source = "f <- function(flag) {\n"
    source += "".join(f"x{i} <- {i}L\n" for i in range(1024))
    source += "result <- " + "if (flag) (" * 24 + "x0"
    source += (") else 0L" if with_else else ")") * 24
    source += "\nresult\n}\nf(TRUE)\n"
    Path("two.R" if with_else else "one.R").write_text(source)
```

For each binary and input, run the following command with a distinct profile
and output path. Also run it with `crates/ry-checker/testdata` as the input,
from the same worktree for both binaries. Compare each pair of output files.
The corpus returns status 1 because its fixtures deliberately contain errors.

```sh
RY_NO_INSTALLED_LIBRARIES=1 RAYON_NUM_THREADS=1 \
  valgrind --tool=dhat --dhat-out-file=profile.json \
  /path/to/ry check one.R > diagnostics.txt
```

The Criterion benchmark covers 128 and 1,024 bindings, with and without `else`:

```sh
cargo bench -p ry-checker --bench performance -- check_if_expression_scopes
```

The recorded comparison uses DHAT rather than Criterion elapsed time. Concurrent
host activity makes elapsed-time comparisons unsuitable for this run.
