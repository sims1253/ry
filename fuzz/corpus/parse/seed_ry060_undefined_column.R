# expect: RY060
# `mtcars` has a known column schema (mpg, cyl, disp, ...), so accessing
# a column name not in the schema emits RY060 (undefined-column).
df <- mtcars
bad <- df$nonexistent
