# oracle: must-pass
# Adjacent Unicode escapes encode UTF-16 pairs in either width/braced form.
stopifnot(identical(utf8ToInt("\uD83D\uDE00"), 128512L))
stopifnot(identical(utf8ToInt("\u{D83D}\u{DE00}"), 128512L))
stopifnot(identical(utf8ToInt("\U0000D83D\U0000DE00"), 128512L))
stopifnot(identical(utf8ToInt("\uD83D\U{DE00}"), 128512L))
stopifnot(identical(utf8ToInt("\U{D83D}\uDE00"), 128512L))
stopifnot(identical(utf8ToInt("\uD800\uDC00"), 65536L))
stopifnot(identical(utf8ToInt("\uDBFF\uDFFF"), 1114111L))
# R warns about unpaired surrogates and produces non-UTF-8 values. The AST
# retains their source spelling; this change does not introduce diagnostics.
for (text in c('"\\uD800"', '"\\uDC00"', '"\\uDC00\\uD800"',
               '"\\uD800x\\uDC00"', '"\\uD800\\u0041"')) {
  value <- suppressWarnings(eval(parse(text = text)))
  stopifnot(is.na(utf8ToInt(value)))
}
stopifnot(inherits(tryCatch(parse(text = '"\\u{D800}\\u{DC00"'),
                            error = identity), "error"))
