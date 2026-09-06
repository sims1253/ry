# oracle: must-pass
quote <- raw()
for (x in as.raw(c(1, 2))) {
  if (length(quote)) {
    if (x == quote) print(x)
  }
  quote <- x
}
