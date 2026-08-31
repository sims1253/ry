# Purpose: dplyr non-standard evaluation. `library(dplyr)` plus the
# `packages = ["dplyr"]` key in ry.toml give the data-mask model, so bare
# column names inside select/mutate/summarise resolve against the data
# frame's schema.
#
# Expected diagnostics: RY010 exactly once - the misspelled column
# `unitss` in the top-level summarise() call at the bottom, where the
# schema of `sales` is known. Note the deliberate contrast: the same typo
# inside units_summary() would be SILENT, because the parameter `df` has
# an unknown schema and unknown-schema column candidates stay silent
# (verified; see README).
#
# LSP probe: fix the typo and the squiggle must clear on the next
# publish; re-break it and it must come back.

library(dplyr)

sales <- data.frame(
  item = c("espresso", "café latte", "cold brew"),
  units = c(10L, 4L, 7L),
  price = c(2.5, 4.5, 3.25),
  stringsAsFactors = FALSE
)

daily_report <- function(df) {
  picked <- select(df, item, units, price)
  mutate(picked, total = units * price)
}

units_summary <- function(df) {
  summarise(
    df,
    n_items = n(),
    total_units = sum(units),
    .groups = "drop"
  )
}

report <- daily_report(sales)
summary <- units_summary(sales)

# Schema is known here, so the misspelling is caught (RY010 on `unitss`).
bad_summary <- summarise(sales, total_units = sum(unitss))

# KNOWN NON-DIAGNOSTIC: tidyselect bare columns are not schema-checked in
# select() (they may be tidyselect helpers, strings, or negative picks),
# so `itemm` here is SILENT even though the schema of `sales` is known.
# Contrast with the summarise() line above, which is checked.
picked_bad <- select(sales, itemm)
