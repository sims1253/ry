# Project check in hooks and CI

Copy [the hook config](.pre-commit-config.yaml) and [ry.toml](ry.toml) to
your project root. Copy [the CI workflow](.github/workflows/ry.yml) to
`.github/workflows/ry.yml`. The workflow installs ry 0.11.0 from its pinned
release URL, verifies the version, and checks the whole project. Update the
release URL and version check together when you upgrade.

Install the same ry release locally with the
[versioned installer](https://github.com/sims1253/ry/releases/tag/v0.11.0),
or put a preinstalled `ry` binary on `PATH`. Then choose one hook runner:

```sh
pre-commit install
# or: prek install
```

Run `pre-commit run ry-project --all-files` or
`prek run ry-project --all-files` to check before a commit. Both runners use
the same YAML file. The local/system hook runs `ry check .` once from the
project root. `pass_filenames: false` keeps the full project context when
only one R file changes. `always_run: true` also checks commits that change
only `ry.toml`, a baseline, stubs, `DESCRIPTION`, or `NAMESPACE`.

The hook can cost more than a file-local linter because it checks every
admitted R source on each commit. During a commit, the runners temporarily
hide unstaged changes to tracked files, so the hook checks their staged
contents. Untracked files can still be visible to a project scan. CI checks
the fresh checkout and is the final check for committed content.

The example `ry.toml` fails on warnings as well as errors. If your project
has reviewed existing findings, run
`ry check --write-baseline ry-baseline.json .`, review and commit that file,
then enable the `baseline` setting. New findings still fail. The workflow
does not use `--exit-zero` or rewrite the baseline.
With `error-on-warning = true`, this first command exits 1 because it reports
the existing findings, even though it writes the baseline file. Inspect the
file before enabling it; a subsequent check should pass only for the reviewed
findings.

For an offline hook, provide a preinstalled binary on `PATH`; neither
the hook nor the checker needs R or Rust. An offline CI runner can supply
the same binary and omit the install step, while retaining the version
check. Keep formatters such as air and style linters such as lintr in
separate hooks; they do not replace this project-level type check.
