# Project refinement trace

`Project` can record bounded, opt-in execution telemetry for its collection,
fixpoint refinement, and diagnostic emission passes. The trace answers which
production scheduling branch caused work and whether the last fixpoint attempt
converged or reached its depth bound. It is not a diagnostic explanation or a
proof of an inferred type.

```rust
use ry_checker::{Project, TraceOptions};

let mut project = Project::new();
// Add parsed files in the same order as the check you want to inspect.
project.enable_trace(TraceOptions::default())?;
let diagnostics = project.check_incremental();
let trace = project.take_trace().expect("trace enabled");
```

Tracing is off by default. `enable_trace` validates its limits and does not
change diagnostic or facts capture settings. `disable_trace` turns it off.
`take_trace` consumes the latest report; checking again replaces an untaken
report. Existing `Project` instances keep their normal collection and emission
caches while tracing is enabled. When tracing begins after an untraced check,
`enable_trace` snapshots the current winning definitions so later removals
retain their previous source identity. Edits and removals after `check()` also
retain trace-only transition names even though that cold entry point does not
keep the pass-1 collection cache. This telemetry does not change refinement
scheduling. Enable tracing before the edit of interest.

For a standalone developer run, pass UTF-8 R files to the example:

```sh
cargo run -p ry-checker --example project_trace -- \
  --max-events 128 --max-event-bytes 16384 R/leaf.R R/caller.R
cargo run -p ry-checker --example project_trace -- \
  --edit R/leaf.R /tmp/edited-leaf.R R/leaf.R R/caller.R
```

The second command prints a JSON array with the initial and edited checks.
The first argument to `--edit` must exactly match a listed logical project
path; the replacement file supplies its contents. `--file-filter PATH` keeps
events for one logical path, while global round and completion events remain.
The API also accepts a `TraceFunctionId` filter obtained from a prior trace.
This example reads files as UTF-8 and is not the CLI's full project discovery
or encoding pipeline.

Each report has schema version 1, a run number monotonically increasing within
one `Project` instance, a SHA-256 input snapshot digest, summary counters, and
an ordered event array. The digest covers ordered logical paths, their source
digests, and declared package names. It does **not** cover external package
stubs or every environment input, so it is not a complete reproducibility
key. A file identity contains its project order, logical path, and source
digest. A function identity names the winning merged-table binding and its
file identity. Function names can be shadowed and identities can change after
an edit; these IDs are valid within the recorded snapshot, not stable symbols
for navigation or rename.

Collection events report pass-1 cache hits and misses. Invalidation, scope,
schedule, changed-function, and emission events are emitted from the branches
that actually select work. A dependency-caused `scheduled` or `emission`
event carries a `trigger`: either the changed function or the return slot read
by the caller. A return slot includes its snapshot-local numeric index and
all winning function bindings sharing it. A file/function filter selects the
event target; a cross-file trigger remains attached. An invalidation targets
the prior winning definition; if removing it exposes an earlier shadowed
definition, the new winner appears as its trigger. A slot changed in both
return type and evaluation metadata reports both in its reason. If several
dependencies could schedule the same target, the stream records the first one selected by
deterministic traversal, not an exhaustive proof of every cause. An unresolved
function or file target is marked by its unresolved name/path and cannot pass
an exact file/function filter as though it were a global event. A `Round`
records pending counts,
function-body refinement attempts (including repeats), and the number of
return and evaluation-metadata changes. The summary's `refined_functions`
also counts attempts, not unique functions or changed results. `FixpointComplete` reports
`converged` or `bound_reached`; the latter means the configured internal depth
limit stopped refinement, **not** that the result converged. A full-scope retry
can produce more than one completion event; the summary completion is for the
last attempt and its round count is the total of attempts. The synchronous
`Project` check API does not accept cancellation, so it never emits
`cancelled`; that status is reserved for a future cancellable entry point.

`TraceOptions` defaults to 512 events and 64 KiB of event-array JSON. Valid
limits are 1–10,000 events and 256 bytes–1 MiB. Limits include the visible
`truncated` marker; the summary reports the number of omitted events and final
event-array byte count. Summary fields and report framing are outside the
event-byte cap. A one-event limit, or a byte limit too small for the first
identity-bearing event plus marker reserve, produces a marker-only stream with
summary counts. Source contents, full ASTs, and environment values are not
included. Paths and function names still appear in events, so handle a report
with the same care as other project metadata.

For comparisons, normalize `analysis_run_id` when separate `Project` instances
have different histories. The ordered logical events are deterministic for
equivalent project order and inputs. Wall-clock duration, thread execution
order, and any optional timings collected outside this trace are not. An
enabled run should be compared with a disabled run using the same parsed files;
the integration tests check diagnostics, scopes, and reference facts parity.

The existing Criterion suite includes `warm_edit_trace/disabled` and
`warm_edit_trace/enabled`. Both arms parse alternating one-line edits to the
same vendored glue source, check the same persistent project, and take the
possibly absent report. JSON export is outside the timed path. Run with:

```sh
RAYON_NUM_THREADS=2 cargo bench -p ry-checker --bench performance -- warm_edit_trace --noplot
```

These local measurements describe the opt-in warm-edit cost on the measured
machine. They are not a default-on budget or a zero-overhead claim.
