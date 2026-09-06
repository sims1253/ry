# Performance tracking

The `Performance` workflow runs on pushes to `main` and manual requests. The full
suite does not run on PR pushes; existing CI budget tests provide the cheaper
PR checks. It measures:

- Core parsing, checking, branch handling, and incremental edits with the existing
  `ry-checker` Criterion suite. Reports use mean time in nanoseconds and include
  the confidence interval.
- Release CLI executable size.
- VS Code activation and activation-to-first-diagnostic latency, plus
  JavaScript bundle size and VSIX size without the bundled server.
- Release Zed WASM size.

Extension sizes measure distribution cost. Zed runtime latency and memory use
are not measured. The incremental core benchmarks model part of the
LSP workload but do not exercise an editor or the JSON-RPC transport.

## Reading results

Each run writes a measurement table to its GitHub Actions summary and uploads
`results.json` and `environment.txt` as a 14-day artifact. The report job compares
against the latest recorded `main` result. The first run establishes a baseline.
A slowdown or size increase above 20% is advisory; it does not fail the workflow.
The existing performance budget and scaling tests in `CI` remain enforced.

Only runs on `main` push history to the separate `performance-data` branch.
Manual runs on other branches compare without publishing history. The workflow
does not post comments. The history contains numeric measurements and
commit metadata in `benchmarks/data.js`, plus a static chart page. No executables,
VSIX packages, WASM files, or Criterion sample files enter git history.
The chart retains the latest 365 runs; older revisions remain in the history
branch as small text diffs.

The workflow uses Ubuntu 24.04 and two Rayon workers. Rust stable, the runner
image, and other tools can change over time; the environment artifact records
Rust, Bun, and CPU details. Hosted runner noise and toolchain changes can affect
results. Confirm a suspected regression with repeated measurements on the same
machine before treating it as a code regression. Changes to benchmark fixtures
also change the workload and require care when comparing results.

## Enable the public dashboard

1. In repository **Settings → Pages**, select **GitHub Actions** as the source.
2. Add the repository Actions variable `BENCHMARK_PAGES` with value `true`.
3. Run `Performance` on `main`, or push a change to `main`.

The dashboard will be at `https://sims1253.github.io/ry/benchmarks/`.
History and run summaries work before Pages is enabled. The deployment publishes
the performance site as the repository's Pages site; coordinate this setting if
another workflow already owns that site. No personal access token is needed.

Charts and comparisons use
[github-action-benchmark](https://github.com/benchmark-action/github-action-benchmark),
pinned to a release commit in the workflow.

## VS Code activation

The benchmark starts five fresh VS Code 1.90.2 extension hosts with separate
profiles and empty workspaces. This fixed version matches the extension's
minimum supported editor series. It explicitly activates the compiled extension
and records the median time, with the minimum and maximum as the range.
Each host uses the locally built release server through `ry.path`.

Activation ends before the extension's deferred server startup. A second metric
therefore measures from activation start until the server reports `RY040` for
`x <- 1 + "a"`. A missing diagnostic fails the benchmark. This includes binary
resolution, server startup, document opening, and the first check. Editor launch
and downloading VS Code are outside both timers. These are fresh-process
measurements with potentially warm OS caches, not cold-disk startup timings.

## Run locally

```sh
RAYON_NUM_THREADS=2 cargo bench --locked -p ry-checker --bench performance -- --noplot
cargo build --locked --release -p ry-cli
rustup target add wasm32-wasip2
cargo build --locked --release --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
(cd editors/code && bun install --frozen-lockfile && bun run vsce-package)
(cd editors/code && xvfb-run -a node test/performance.cjs /tmp/ry-activation.json)
python3 scripts/performance/collect.py --activation /tmp/ry-activation.json --output /tmp/ry-performance.json
```

Use a clean checkout without `editors/code/bundled/bin/ry` when measuring the VSIX
without a server. Remove `target/criterion` before running a changed benchmark
suite so deleted benchmarks do not remain in the local report.
