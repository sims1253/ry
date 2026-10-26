# no-diag
page <- if (requireNamespace("bslib", quietly = TRUE)) bslib::page_fluid else shiny::fluidPage
fallback <- if (!requireNamespace("bslib", quietly = TRUE)) shiny::fluidPage else bslib::page_fluid
probe <- function(pkg) if (!requireNamespace(pkg, quietly = TRUE)) bslib::page_fluid else shiny::fluidPage
guarded <- if (!requireNamespace("bslib", quietly = TRUE)) tryCatch(bslib::page_fluid, error = function(e) shiny::fluidPage) else shiny::fluidPage
