# set6 and dictionar6 import inventories

Issue [#366](https://github.com/sims1253/ry/issues/366) reports unbound names
from wholesale imports in param6 and distr6. ry parses those imports, but needs
the dependency's export inventory to resolve each name. The bundled stubs now
supply all exports from set6 0.2.4 and dictionar6 0.1.3.

```text
NAMESPACE import(set6)
  → bundled exports: Set, Reals, Interval, ...
  → imported names resolve without an installed set6
  → Reaals still produces RY010
```

R6 generators such as `Set` and `Dictionary` are opaque values. Function
formals remain inference-only. The stubs make no return-type, length, or
missingness claims. The r-typeshed runtime test compares all exports and
formals with the archived R packages and checks R6 construction.

## Reported packages

The comparison used one binary built from tracked ry commit `92c822a` and
stubs from r-typeshed commit `cadfe7a035ea06dec3898d3a15c8d8fefb5df0cc`.
Installed-library lookup was disabled with `RY_NO_INSTALLED_LIBRARIES=1`.
Neither package had project configuration or globals overrides. Both runs
used the same package-root discovery and compared all diagnostic JSON fields.
All files in the checked source directories matched their CRAN archives.

| Package source | Baseline diagnostics | With inventories | Removed RY010 |
| --- | ---: | ---: | ---: |
| param6 0.2.4 | 133 | 0 | 133 |
| distr6 1.6.9 | 191 | 10 | 181 |

All 314 removed findings name verified exports from the two new inventories.
No diagnostic was added. The remaining distr6 findings are eight RY010s and
two RY100s; every diagnostic is unchanged from the baseline. These counts
describe the pinned sources, not other versions of the packages.

The source archive SHA-256 checksums are:

```text
param6_0.2.4.tar.gz   4d06afac7b9c228a0706c8af098853f001e68b3b754dc5005e7a7f700c159940
distr6_1.6.9.tar.gz   10b7545813be12cdefcf5ea9465845d6ca90dd75365fd35b4f263c90c1eb88ee
set6_0.2.4.tar.gz     099cdadbca907cc72777b642c1c59c9089ac6121256cbdf3a9478d7381f95c1b
dictionar6_0.1.3.tar.gz 2e55026e43adf8dc6929bd9c09a5e67bc57d148560a0d1e2840f5fe22f2ae9e5
```

To replay with the baseline binary and a checkout of the stub commit:

```sh
RY_NO_INSTALLED_LIBRARIES=1 ry check "$package_path" --output-format json
RY_NO_INSTALLED_LIBRARIES=1 ry check "$package_path" --output-format json \
  --typeshed "$typeshed_checkout/stubs/set6" \
  --typeshed "$typeshed_checkout/stubs/dictionar6"
```

The CLI regression test runs without installed-library lookup. It checks
wholesale imports, selective `importFrom` declarations, and an absent import,
with `Reaals` as a misspelling that must remain reportable. This change supplies
two missing inventories; other unavailable dependencies still need installed
metadata, local stubs, or reviewed project globals.
