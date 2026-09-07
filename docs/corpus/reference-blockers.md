# Reference blocker provenance panel

[Issue #294](https://github.com/sims1253/ry/issues/294) asked why unavailable
reference facts share `unsupported_scope`. The [pinned results](reference-blockers-2026-09-07.json)
compare baseline `0e658b9` with implementation `6f14dcd`. All existing reference
statuses, reasons, definition IDs, types, scope facts, and diagnostic JSON are
unchanged across 80 cases. The only new semantic payload is `blocker`.

The supplied discovery panels each contain 27 files with the **same 27 source
hashes**. Their 54 entries therefore do not provide 54 independent sources.
Each panel exports 4,795 reads; together they export 9,590. This is the count
for the frozen inputs and baseline recorded here, rather than the issue's
earlier 9,454 count. The separate eight-window suite exports 745 reads. Every
read in these selected real-source panels remains `unsupported_scope`; this
change explains existing refusals and adds no resolved references.

| Primary blocker | Each 27-file discovery panel | Eight windows |
| --- | ---: | ---: |
| Containing unsupported statement | 216 | 67 |
| Prior unsupported statement | 1,480 | 385 |
| Inherited unsupported statement | 1,014 | 33 |
| Whole-scope formal write | 1,151 | 205 |
| Inherited formal write | 321 | 53 |
| Whole-scope unsupported formal | 583 | 0 |
| Lazy default expression | 22 | 2 |
| Inherited lazy default | 8 | 0 |
| **Total** | **4,795** | **745** |

These counts use the documented [primary-blocker precedence](../facts.md#blocker-provenance).
They do not count every applicable restriction or estimate general R-code
coverage. An inherited restriction keeps the original ancestor's cause and
location; it does not label the body read as unsupported syntax.

The existing [ordered-reference panel](reference-prefix-panel.json) adds 15
authored cases and three vendored files. Its 127 reads retain exactly 19
resolved records, 105 `unsupported_scope` refusals, two `after_unsafe_read`
refusals, and one `unbound_name` record. The two unsafe-read refusals now point
to the first unsafe read. The 19 resolved records and unbound record have no
additional blocker.

## Method and validation

The discovery inputs came from the supplied `sepalith-discovery-glm-20260907`
and `sepalith-discovery-muse-20260907` panels. The eight source windows came
from `sepalith-autoresearch-20260907/suite.json`. Inputs were copied read-only
into isolated directories as unchanged UTF-8 `source.R` files, each next to an
empty `ry.toml`. The report pins each source hash and its supplied origin.
The fixed repository panel supplies the remaining source texts or vendored
paths. These standalone runs do not reproduce the original package contexts.

Each frozen binary ran these commands independently for each source, with
`RY_NO_INSTALLED_LIBRARIES=1` and `RAYON_NUM_THREADS=1`:

```sh
ry dump-facts source.R --references --format json
ry check source.R --output-format json
```

The comparison removes only `blocker` and executable-derived identity fields:
`producer.executable_hash`, context IDs, `contexts[].inputs.build.executable_hash`,
and `contexts[].inputs.typeshed.bundled_build_hash`. Every other facts field
and every diagnostic is compared unchanged. Blocker spans are checked against
the original UTF-8 bytes. Repeating all 80 candidate exports in fresh processes
produces byte-identical JSON, including blocker fields.

CLI regressions cover the three issue examples, first unsafe formal reads,
semantic inheritance from an outer formal read, separate lazy-default spans,
whole-scope precedence, and original ancestor owners. They include Unicode
identifiers, CRLF, and tabs. Existing tests also compare inference and
diagnostics with reference capture disabled. The workspace, Clippy, formatting,
and complete R oracle gates pass.
