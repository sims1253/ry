# oracle: must-flag
# Base defines no `$.Date`, so `$` on this classed double falls back to the
# primitive and errors as it does on a plain double.
today <- Sys.Date()
today$year
