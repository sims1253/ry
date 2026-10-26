# Serialized workspace boundary fixtures

These small R version-2 workspaces are committed as bytes so Rust CLI and LSP
tests exercise the serialized reader without invoking R at test time. They were
created with Rscript 4.6.1 using:

```r
x <- NULL
for (i in seq_len(70L)) x <- list(x)
save(x, file = "nested-limit.rda", version = 2, compress = TRUE)
x <- NULL
for (i in seq_len(300L)) x <- list(x)
save(x, file = "nested-deep.rda", version = 2, compress = TRUE)
save(list = character(), file = "empty.rda", version = 2, compress = TRUE)
save(list = character(), file = "empty-ascii.rda", ascii = TRUE, version = 2)
```

`nested-limit.rda` is 63 compressed bytes (well below the default decoded-byte
cap) and exceeds the parser's default nesting limit of 64; the skipping tag
walker still enumerates it. `nested-deep.rda` (300 levels) exceeds the
walker's depth cap too and stays a parser resource limit. `empty.rda` is a
valid workspace containing no bindings.
`empty-ascii.rda` is also a valid empty workspace. Its ASCII envelope is
unsupported by the XDR-only static reader and must be reported as unsupported,
not malformed.

`extptr.rda` binds `a_first`, an external pointer `ptr_value`, and `z_last`
(#714). It was created with Rscript 4.5.0 using:

```r
a_first <- 1
ptr_value <- methods::new("externalptr")
z_last <- 2
save(a_first, ptr_value, z_last, file = "extptr.rda", version = 2, compress = TRUE)
```

The external pointer's payload (its protected value and tag) follows its type
word. rds2rust 0.3 does not read that payload and ends the workspace after
`ptr_value`; the tag walker in `serialized.rs` reads past it, so all three
names enumerate.

`extptr-nested.rda` carries the pointer in the first object's attributes
(the shape that hid later gtsummary bindings), then an ALTREP sequence, a
byte-compiled function, and an environment before a final binding. It was
created with Rscript 4.6.1 using:

```r
df_like <- data.frame(x = 1:2)
attr(df_like, "problems") <- methods::new("externalptr")
seq_like <- 1:1000
fn_like <- compiler::cmpfun(function(v) v + 1)
env_like <- new.env(parent = emptyenv())
env_like$inner <- TRUE
z_last <- "after"
save(df_like, seq_like, fn_like, env_like, z_last,
     file = "extptr-nested.rda", version = 3, compress = TRUE)
```
