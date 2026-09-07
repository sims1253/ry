# S4 initializer proof boundary

Status: proposed groundwork for [#268](https://github.com/sims1253/ry/issues/268).
This document does not enable an inference path or an analysis option. The
original ignored-dots example remains a known gap. A future implementation
must establish a trusted registry contract before using the state below.

## Observed behavior

R 4.6.1 accepts the example in [gap_new_initializer_dots.R](../../crates/ry-checker/testdata/oracle/gap_new_initializer_dots.R): an initializer
whose body is `.Object` does not force `new()` dots. The checker currently
visits those expressions and reports RY040.

The apparent fix—remember the latest literal `setMethod()` body—does not
establish the method that R installs. Registration itself executes S4 code:

- `setClass()` constructs metadata through
  `newClassRepresentation()` → `new("classRepresentation", ...)`.
- Slot prototype construction can call `tryNew()` → `new(slotClass)`.
- `setMethod()` converts a closure through `asMethodDefinition()` →
  `new("MethodDefinition", def)` and constructs a `signature` object.

These calls can invoke user initializers. An incomplete source inventory does
not prove that those initializers are absent. Initializers may change caller
bindings, install active or delayed bindings, or replace subsequent registry
entries. Consequently, successful registration is not a purity certificate.

The oracle [new_initializer_metadata_replacement.R](../../crates/ry-checker/testdata/oracle/new_initializer_metadata_replacement.R) replaces the
`MethodDefinition` initializer. It delegates the usual construction, then
sets the resulting method's `.Data` to a function that forces dots. A later
literal ignored-dots definition is installed with this forcing body. The
fixture checks the installed body before reaching the expected arithmetic
error. This demonstrates a changed method, rather than a failed class setup.

The probes used `R version 4.6.1 (2026-06-24)` and methods 4.6.1.
The installed implementation can be inspected without executing a project:

```sh
Rscript --vanilla -e 'cat(R.version.string); print(methods::setClass); print(methods::setMethod); print(methods:::asMethodDefinition); print(methods:::newClassRepresentation); print(methods::new); print(methods::getMethod)'
```

The call chains above were checked against those function bodies. They are
implementation evidence for this R version, not a cross-version contract.

## Why a method query is not a certificate

A candidate source assertion is:

```r
stopifnot(identical(
  body(methods::getMethod("initialize", "W")),
  quote(.Object)
))
methods::new("W", "ignored" + 1L)
```

[new_initializer_query_is_not_certificate.R](../../crates/ry-checker/testdata/oracle/new_initializer_query_is_not_certificate.R) demonstrates why this is
insufficient. An active binding in the method table removes itself, registers
a forcing method, and returns the earlier ignored-dots method. The assertion
passes; `new()` reaches the forcing method and errors. `selectMethod()` also
returns a value, not a guarantee about the table after lookup.

A runtime assertion route would therefore need to establish all of these
properties, with proven callee identities and evaluation order:

1. The queried generic, class definition, table, and analysis environment are
   the same objects used by the later constructor.
2. Every relevant lookup uses ordinary bindings, with no active bindings or
   unforced promises that can change those objects during lookup.
3. Method selection returns the installed method, including its formals,
   closure environment, and forcing behavior, for that class identity.
4. No intervening evaluation or constructor lookup invalidates these facts.

Checking only a body, a class name, or a method returned by a query does not
establish these properties. The checker has no existing contract that proves
all four. This proposal therefore does not recognize the assertion above.

## Missing root contract

The trust source must be chosen before designing a contract format. No
existing invocation opts into a pristine registry. A clean source prefix,
installed package version, or fresh-R observation cannot establish the
registry of an arbitrary package or interactive session.

A future provider must warrant the registry and binding identities, lookup
behavior, and metadata initializer contracts for the analysis environment.
A contract fingerprint can identify that assertion; it cannot prove the
runtime satisfies it. Runtime assertions remain another possible route, but
only if their own resolution and effects establish the complete boundary
above.

The first initializer summary should require exact formals `(.Object, ...)`,
no defaults, body `.Object`, and a known closure environment. Registration
contracts must cover metadata and slot-prototype dispatch. Apply their caller
effects before recording a postcondition. If a postcondition does not prove
which body was installed, it provides no ignored-dots fact.

## Ordered state

Each scope needs separate registry facts; the pooled `s4_methods` inventory
must remain insufficient evidence. The minimal proof key is:

```text
(environment identity, generic identity, class identity, class generation,
 method generation, registry contract identity)
```

The value records the applicable initializer identity, its forcing summary,
its effect summary, and all metadata contracts used to establish it. Class
names are labels within an environment, not identities. Function bodies in
other scopes and later registrations cannot satisfy the key.

| Operation | State transition |
| --- | --- |
| Proven class registration | Match real formals, evaluate arguments in runtime order, apply all registration effects, then install a fresh class generation only if the contract proves the resulting class identity. |
| Proven method registration | Match `f`, `signature`, `definition`, and `where`; apply conversion and registration effects; install a new method generation only if the postcondition proves the resulting method and its class/environment identity. |
| Removal or redefinition | Remove the affected certificate; class redefinition also invalidates certificates tied to its previous generation. |
| Unknown call, lookup, assignment effect, or dynamic registration | Discard registry certificates that may be affected; lack of an effect contract means all certificates are affected. |
| Branch merge | Retain only identical certificates from every continuing branch. |
| Loop or deferred function body | Do not borrow a certificate from a different execution point; start unknown unless entry and iteration contracts establish it. |
| `new()` | Force and match `Class` first, resolve the actual class identity, then consult the current certificate. Skip dots only with a valid ignored-dots certificate. Apply initializer effects even when dots are skipped. |

The caller-binding state and the registry state are distinct. Invalidating
caller values must also invalidate any registry facts those effects can
change. Reassigning a literal does not erase the possibility of an active
binding installed by earlier code. Conversely, default initialization must
retain its existing useful analysis when no new uncertainty has been proved;
this work must not make every S4 file opaque after `new()`.

## Executable controls and next decision

The following oracle fixtures run each program in a fresh R process:

| Fixture | R outcome |
| --- | --- |
| [gap_new_initializer_dots.R](../../crates/ry-checker/testdata/oracle/gap_new_initializer_dots.R) | Succeeds; existing checker gap remains. |
| [new_before_initializer_registration.R](../../crates/ry-checker/testdata/oracle/new_before_initializer_registration.R) | Errors before the later method registration. |
| [new_after_initializer_removal.R](../../crates/ry-checker/testdata/oracle/new_after_initializer_removal.R) | Errors after removal restores forcing behavior. |
| [new_masked_initializer_registration.R](../../crates/ry-checker/testdata/oracle/new_masked_initializer_registration.R) | Errors because the masked registration did not install a method. |
| [new_initializer_metadata_replacement.R](../../crates/ry-checker/testdata/oracle/new_initializer_metadata_replacement.R) | Errors after metadata construction replaced the literal body. |
| [new_initializer_query_is_not_certificate.R](../../crates/ry-checker/testdata/oracle/new_initializer_query_is_not_certificate.R) | Errors even though the method-body assertion passed. |

Run them through the existing complete oracle harness:

```sh
cargo test -p ry-checker --test oracle -- --include-ignored
```

Before wiring the state into inference, select a product-supported source for
the root contract, or implement an assertion that proves the complete lookup
boundary above. Neither an implicit pristine registry nor a query-result
shortcut is an acceptable substitute. Then add caller-effect, dynamic
registration, environment mismatch, and branch/loop invalidation controls
alongside the acceptance oracle and require red/green evidence for each
state transition.
