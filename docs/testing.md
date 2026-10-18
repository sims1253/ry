# End-to-end testing

Use complete user journeys as acceptance tests for changes to CLI and editor
behavior. Keep focused tests for R semantics, generated inputs, and failures
that a user journey cannot cover efficiently. Before implementing an isolated
test, list the relevant failure modes and their observable outcomes.

## Installed-extension journey

The Linux installed-VSIX suite runs the packaged extension and bundled `ry` binary in
fresh trusted and untrusted VS Code profiles. It uses VS Code APIs to open and
edit documents and apply the server's code actions. It does not automate clicks
or verify the rendered editor UI.

The diagnostic journey covers these failures:

- The packaged extension fails to activate or starts the wrong binary.
- A diagnostic has the wrong code, range, or severity.
- Fixing a source line leaves a stale diagnostic.
- Undo fails to restore the diagnostic.
- A suppression action is missing, edits the wrong line, or suppresses an
  unrelated error.
- A saved suppression stops working after the document closes and the server
  restarts.
- The untrusted profile executes a workspace-supplied binary.

Each diagnostic checkpoint records the source, expected diagnostics, and
observed diagnostics. After a suppression, another invalid line must still
produce its diagnostic. This verifies that the server analyzed the document
instead of treating an empty diagnostic collection as success.

## Run the suite

From the repository root, build the binary and install the extension's locked
dependencies:

```sh
cargo build -p ry-cli
cd editors/code
bun install --frozen-lockfile
mkdir -p bundled/bin
cp "$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')/debug/ry" bundled/bin/ry
bun run vsce-package
xvfb-run -a bun run test
```

On a machine with a display, omit `xvfb-run -a`.

The runner writes `editors/code/test-results/installed/` on success and failure.
Each run replaces the previous results. The directory contains:

- `manifest.json`: commit, working-tree status, Node and editor versions, VSIX
  and bundled-binary SHA-256 hashes, and overall run status.
- `ry.vsix`: the package installed by the run.
- `trusted/` and `untrusted/`: test results, checkpoint JSON files, editor
  process output, the final workspace, and VS Code profile logs for profiles
  that started.
- `rerun.sh`: rerun the saved VSIX with the recorded editor version from this
  checkout. If the editor download fails, the script retains the requested
  version instead.

To reproduce a run, check out the commit in `manifest.json`, install the locked
dependencies, and run `bash /path/to/rerun.sh /path/to/checkout`. The checkout
argument defaults to the current directory. The rerun needs network
access if that editor version is not cached. A dirty working tree is recorded
in the manifest and must be reproduced separately.

To verify the saved package and binary, run these commands in the evidence
directory and compare their output with the hashes in `manifest.json`:

```sh
sha256sum ry.vsix
unzip -p ry.vsix extension/bundled/bin/ry | sha256sum
```

CI uploads this directory even when the suite fails. Checkpoint assertions and
exit status determine success. The saved files let a reviewer inspect the
result and repeat the scenario. Timings, process IDs, and temporary paths can
differ between runs.
