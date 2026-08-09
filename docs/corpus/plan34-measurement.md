# Plan 34 pre-governance measurement

## Run provenance

The unmodified starting tree was measured at audited commit
`a2430489a9bbf65215b8d5fbe739345bd34d1b15`. The release binary was rebuilt
from that commit before the corpus run (rather than trusting a pre-existing
`target/release/ry`). The full 62-package corpus was run hermetically with
`RY_NO_INSTALLED_LIBRARIES=1` using `ecosystem/posit-packages.txt` and
reconciled identity-by-identity with `docs/corpus/posit-0.8.0.json`.

The measured identities are committed in `posit-plan34-baseline.json`; its
source-report SHA-256 is `602fe749c12cfb2217d96fdb82a93634907a4f1d4f9507a27abb51d5209fb7e7`.

## Measurement

| Population / metric | Result |
| :-- | --: |
| Original 0.8.0 identities | 1,142 |
| Surviving identities | 338 |
| Resolved identities | 804 |
| New identities (manually classified) | 391 |
| Current diagnostics | 729 |
| Current TP / FP / uncertain | 37 / 692 / 0 |
| Overall precision | 5.08% |
| `R/` diagnostics | 547 |
| `R/` TP / FP | 23 / 524 |
| `R/` precision | 4.20% |
| Original TP retained | 31 / 34 (91.18%) |

Plan 31 projected approximately 91 total diagnostics, approximately 29%
overall precision, 43 `R/` diagnostics, and approximately 44% `R/` precision
after its suppression work. The measured tree instead emits 729 diagnostics
at 5.08% overall precision and 547 `R/` diagnostics at
4.20% precision. The projection therefore did not materialize under the
hermetic corpus. Most of the difference is visible rather than hidden: hermetic
dependency bindings produce 314 new RY010 identities, while the targeted RY032
parameter-vector heuristic accounts for 47 newly emitted identities.

## Original true-positive retention

Exactly three original true-positive identities were resolved. All three are
intentional bad-code fixtures, not upstream production defects, but they are
named here because the 0.8.0 classification called them true positives:

- `lintr / RY034 / tests/testthat/default_linter_testcode.R:31:73`
- `lintr / RY034 / tests/testthat/dummy_projects/project/default_linter_testcode.R:24:73`
- `styler / RY000 / tests/testmanual/addins/r-invalid.R:1:2`

The other 31 original true positives survive unchanged. This is 91.18% identity
retention; no production-source (`R/`) true positive was lost.

## New findings by rule

| Rule | New | TP | FP |
| :-- | --: | --: | --: |
| RY010 | 314 | 0 | 314 |
| RY030 | 12 | 0 | 12 |
| RY032 | 47 | 0 | 47 |
| RY070 | 2 | 2 | 0 |
| RY102 | 1 | 1 | 0 |
| RY103 | 2 | 2 | 0 |
| RY105 | 13 | 1 | 12 |


Every new identity was reviewed at its source location. The six new true
positive identities are:

- `pkgdown / RY070 / R/templates.R:57:5` (a local `path <- NULL` shadows the function);
- `rlang / RY070 / tests/testthat/helper-locale.R:134:5` (a local logical `skip` shadows the function);
- `pak / RY102 / R/pak-sitrep-data.R:41:5` (the quoted list name uses `<-`);
- `pak / RY105 / R/confirmation.R:42:14` (`length(sum(...)) > 0` is constant);
- `sparklyr / RY103 / R/worker_apply.R:522:40` (multi-class equality in `if`);
- `sparklyr / RY103 / java/embedded_sources.R:2334:40` (the embedded copy).

The other new identities are classified false positive individually in the
ledger. RY010 names are imported/generated/data bindings that exist at runtime;
RY030 sites are dplyr data-mask columns; RY032 reports only unknown parameter
length (not actionable under P34-W2 policy); the other RY105 sites over-narrow callbacks, vectors, or
intentional assertions.

## Package refs

The immutable refs below are the exact refs in the measured manifest.

| Package slug | Commit |
| :-- | :-- |
| `ggplot2` | `6870419aa6e106c3580c45c81d5b688cb31758bd` |
| `shiny-r` | `ca1800475ac5a1e6037fea1020260a868d8798ee` |
| `dplyr` | `d5e94e7fa8fd4a5f79c1a707d1842216bb4c691f` |
| `gt` | `caa074012b9825ddd71996f40e309c00f7b9e180` |
| `blogdown` | `07f3de89f672c0155149fd657a0946c29684a7f8` |
| `reticulate` | `afe2221ded74a7f18ab3d5e89bb0a49b1f9d3114` |
| `broom` | `6230a9014b0d839e4e8ed5b1763de65dde8f2205` |
| `plumber` | `393920505f289f914150d57e06d97347556a55dc` |
| `purrr` | `481e829f297fd4315b386518215157f361475ad0` |
| `tensorflow` | `fed1fe79742d9a979884b07f69c126b91a93050b` |
| `lintr` | `990e57870498634ce67d1fa0e53311f385b4b3d4` |
| `renv` | `a3f8952cf8f4b9cd6e676805431cb81ca5a24b64` |
| `tinytex` | `db1fc10763fa637f758a75ec24e070d82920bbce` |
| `httr` | `34b956569ebaa20cd955150fad765cd834f38be4` |
| `sparklyr` | `7f006a6d5c01a4e7a36d203d1ace56c53261900e` |
| `testthat` | `9b6f12b9f50c297b4b5f485f728a2a19305770eb` |
| `usethis` | `5f60819dc5dee7071d6e67cfa1fd934bb665466c` |
| `keras3` | `753ffeeb4b9a76ae37388cf63be64a45a5a8ef20` |
| `flexdashboard` | `8ae8623f2d9cdadb2316b953908bed0ba7bc3f87` |
| `pak` | `cdbd4ceb07f5a7eb2b317c57237dad561e5fb4e7` |
| `lubridate` | `8e9f2b289ee8384d9499e2527fb1e449c97e5370` |
| `infer` | `0b9c910e658a5d4ed9f6b13e4978b9c23a225cad` |
| `pkgdown` | `179cfaee3d4e17eaa9e7d7c700b1269dc62e4d46` |
| `styler` | `d3d7854df2d73f539b6cb2286dbf5dddaf3e58f6` |
| `glue` | `da9c73f7a3de6a27f3103cb5bb2355820a4c3a6a` |
| `cli` | `86bdefe1eb23399499b40884f7fab84194283bc1` |
| `parsnip` | `b992bc4f13bdb70d661dd629afee615d3711dd80` |
| `roxygen2` | `81691b0f075f707b70b443131a0edcca065d5fbb` |
| `recipes` | `92704cb072b1bb4951061034be6bfa4678e0e43a` |
| `ellmer` | `f8281bed083ff0228e9064c38fb2164cd594d3f1` |
| `corrr` | `aa0488f1a26e0e04dda9e08535719e8f6b6f9154` |
| `rlang` | `1d0e00659b628c07c93c362687550727597ef09f` |
| `torch` | `c67c2fb2909fc77e8fc127ec2867e1a551ee1bf3` |
| `bigrquery` | `fdb0f1eaebc6b7bab66d3727b2817f966d55e473` |
| `dbplyr` | `f478e2070c9a60f4aebcc94132048312209dd097` |
| `bookdown` | `92e702a5133adcbce1f7e3f35cd9700bcbb370b6` |
| `rmarkdown` | `194c2a11e3d345e294906152b6f60f0439422022` |
| `devtools` | `5a10248bb3b727f84a34e517b92fff118652be1e` |
| `tidyverse` | `0231aafbc56914ee5371dd6c7b60677f168d7154` |
| `rticles` | `2e55e747dacb65c2645cc0273db7f7374a1bb1f2` |
| `rvest` | `6c955c0ecfbb0f0bdb1891f0ef952af388b51851` |
| `tidyr` | `26f83e89a690b6cf31a260489b828df0ff43ebb2` |
| `actions` | `aae88a2d9cd3d63571eb05980b09f44611788515` |
| `readr` | `238ea873fdc1a34b3638f01493bd8df9f770ac62` |
| `magrittr` | `73d66ee89c1079d66d235bfde551b6110c934259` |
| `pagedown` | `4fbb005adb8e666d43896ef577e3d2de2b0544e5` |
| `shinydashboard` | `b6d9ca7fb0d9572cd91a8e97f513ee66d610f9af` |
| `leaflet` | `e568bdaba32010e76dffd7d9de7b7299ec7d1809` |
| `tidymodels` | `35bb4145b32999a651e3b6dba2b7bc2f731b3663` |
| `tibble` | `b5e7406e83d8c3291573dbd1ae06bbb9f87b6aac` |
| `readxl` | `47f8aeac0a99eee6c6db2d64ead2225e5e3ae4af` |
| `reprex` | `0dcf301940c050e1cf32f230cab45503d85ac5a4` |
| `learnr` | `e7164a86172200cdfeb53c5f17382e1f86e3e78a` |
| `dtplyr` | `bffe46e664d62021e47251c158c9f02c4cd30400` |
| `stringr` | `ae054b1d28f630fee22ddb3cb7525396e62af4fe` |
| `multidplyr` | `e960a1e83f6a0dded147366053b1d057d46ad2f4` |
| `vroom` | `56e0ce23bd40de8acc3daacc0e85924b7c18d766` |
| `dt` | `6f957f3126b2999e2f65cdeac942d930fb34b8ea` |
| `blastula` | `8b8a6f97ec3aa4cf605321ed3ee7a65c31dc4b24` |
| `bslib` | `f74283b4782f0871254d21b3edf59d84a621b1d2` |
| `forcats` | `f83e0e682d9f874d066630ff78eb12586b5b2a32` |
| `r2d3` | `becfb81989c7fabfe79dee2dde999190025d4ba3` |

## P34-W2 before/after

P34-W2 removed the unused `unknown_is_actionable` parameter and unreachable
`Length::Unknown` emission arm. It retained the narrower, separately implemented
parameter-pattern heuristic; the claim fixture and checker regression establish
that a bare unknown-length parameter remains quiet, while a known length greater
than one diagnoses.

Both the fast (35-package) and full (62-package) hermetic corpus runs reconciled
against `posit-plan34-baseline.json` after the fix. The before/after diagnostic
identity delta is exactly zero: 729 diagnostics overall, with every baseline
identity retained and no unowned identity.
