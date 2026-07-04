# Audit typeshed for fabricated entries.
#
# Walks every name in base_r.json and asserts it exists in a vanilla R
# session. Catches hallucinated typeshed entries.
#
# This script is base-R only: it does not depend on jsonlite (or any
# other package) and never touches the network. We only need the
# top-level function-name keys of the "functions" object, which we
# extract with a line-based regex instead of parsing the full JSON.
#
# Run: Rscript scripts/audit_typeshed.R

# Locate the typeshed data relative to this script so the audit works
# regardless of the caller's working directory.
script_dir <- tryCatch({
  args <- commandArgs(trailingOnly = FALSE)
  file_arg <- grep("^--file=", args, value = TRUE)
  if (length(file_arg) == 1) {
    normalizePath(dirname(sub("^--file=", "", file_arg[1])))
  } else {
    "."
  }
}, error = function(e) ".")

data_path <- file.path(script_dir, "..", "crates", "ry-typeshed", "data", "base_r.json")
data_path <- normalizePath(data_path, mustWork = TRUE)

# Read the file as plain text and extract the function-name keys. The
# structure is:
#   {
#     "version": "0.0.1",
#     "functions": {
#       "c": { ... },
#       "length": { ... },
#       ...
#     }
#   }
# Function names are the quoted keys at indent level 2 (4 spaces)
# inside the "functions" object, each followed by ": {".
lines <- readLines(data_path)

func_start <- grep('^  "functions": \\{', lines)
if (length(func_start) != 1L) {
  stop("Could not locate a single 'functions' object in ", data_path)
}

name_pattern <- '^    "([^"]+)": \\{'
close_pattern <- '^  \\}'

names_all <- character(0)
for (i in seq.int(func_start[1L] + 1L, length(lines))) {
  line <- lines[i]
  if (grepl(close_pattern, line, perl = TRUE)) {
    break
  }
  m <- regmatches(line, regexec(name_pattern, line))[[1]]
  if (length(m) >= 2L) {
    names_all <- c(names_all, m[2])
  }
}

if (length(names_all) == 0L) {
  stop("No function names extracted from ", data_path)
}

failures <- character(0)

# A name resolves if it is directly visible on the search path, or (for
# names of the form "<generic>.<class>") it is a registered S3 method
# resolvable via getS3method. The latter are real base-R functions even
# though they are not exported onto search(). Split on the LAST dot,
# since class names can themselves contain dots.
s3_pattern <- "^[A-Za-z.][A-Za-z0-9._]*\\.[A-Za-z.][A-Za-z0-9.]*$"
name_exists <- function(name) {
  if (exists(name, where = search())) {
    return(TRUE)
  }
  if (grepl(s3_pattern, name, perl = TRUE)) {
    all_dots <- gregexpr("\\.", name, perl = TRUE)[[1]]
    dot_pos <- all_dots[length(all_dots)]
    generic <- substr(name, 1, dot_pos - 1)
    klass <- substr(name, dot_pos + 1, nchar(name))
    m <- tryCatch(
      getS3method(generic, klass, optional = TRUE),
      error = function(e) NULL
    )
    if (!is.null(m)) return(TRUE)
  }
  FALSE
}

cat(sprintf("Auditing %d names from %s\n", length(names_all), data_path))

for (name in names_all) {
  ok <- name_exists(name)
  if (ok) {
    cat(sprintf("PASS: %s\n", name))
  } else {
    cat(sprintf("FAIL: %s (not found in a vanilla R session)\n", name))
    failures <- c(failures, name)
  }
}

cat("\n")
if (length(failures) == 0) {
  cat("All names verified: no fabricated entries.\n")
  quit(status = 0)
} else {
  cat(sprintf("FAIL: %d fabricated name(s) found:\n", length(failures)))
  for (f in failures) cat(sprintf("  - %s\n", f))
  quit(status = 1)
}
