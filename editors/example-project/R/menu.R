# Purpose: CLEAN typed code plus cross-file reads. Reads TAX_RATE and calls
# price_with_tax() from R/prices.R; defines order_total() and top_seller(),
# which R/resolution.R calls. All cross-file names resolve -> ZERO
# diagnostics. (LSP probe: temporarily rename TAX_RATE in prices.R and watch
# this file's RY010 appear / disappear on save.)
#
# Expected diagnostics: none.

menu <- data.frame(
  item = c("espresso", "café latte", "cold brew"),
  price = c(2.5, 4.5, 3.25),
  stringsAsFactors = FALSE
)

#' Gross total for one order line.
order_total <- function(units, item_price, tax_rate = TAX_RATE) {
  round(units * item_price * tax_rate, digits = 2L)
}

#' Most-ordered item by units.
top_seller <- function(units_by_item) {
  names(sort(units_by_item, decreasing = TRUE))[1L]
}

mean_price <- function(prices) {
  if (length(prices) == 0L) {
    NA_real_
  } else {
    sum(prices) / length(prices)
  }
}

# Sequential, typed base-R transforms: control flow the checker must accept.
menu_with_tax <- menu
menu_with_tax$price <- vapply(
  menu$price,
  function(p) price_with_tax(p),
  numeric(1)
)

for (i in seq_len(nrow(menu))) {
  menu_with_tax$price[i] <- order_total(1L, menu$price[i])
}

switch(size <- "medium",
  small = 0.8,
  medium = 1.0,
  large = 1.25
)
