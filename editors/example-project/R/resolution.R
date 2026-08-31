# Purpose: project-wide name resolution.
# 1. Cross-file call: order_total() is defined in R/menu.R and resolves
#    silently here (no false RY010).
# 2. NAMESPACE import: map_dbl is bound by `importFrom(purrr, map_dbl)` in
#    NAMESPACE, with no library(purrr) anywhere -> clean.
# 3. KNOWN NON-DIAGNOSTIC: daily_revenue() does not exist anywhere. ry does
#    NOT flag unknown callees (it cannot prove the name is not provided
#    dynamically), so this line is silent on purpose.
# 4. `walk` is a purrr name that is NOT in the NAMESPACE and purrr is not
#    attached -> RY010 on the value reference.
#
# Expected diagnostics: RY010 (line with `walker <- walk` only).

lines <- list(
  c(2L, 2.5),   # units, unit price
  c(1L, 4.5),
  c(3L, 3.25)
)

totals <- vapply(lines, function(l) order_total(l[1], l[2]), numeric(1))

per_unit <- map_dbl(lines, function(l) l[1] * l[2])

# Exists nowhere in the project: silent (see header note 3).
revenue <- daily_revenue(totals)

# KNOWN NON-DIAGNOSTIC: order_total() has required formals (units,
# item_price) but the project-wide check path does not argument-check
# user-defined functions, so this missing-everything call is silent too.
short <- order_total()

# purrr export, but not named in NAMESPACE and not attached.
walker <- walk
