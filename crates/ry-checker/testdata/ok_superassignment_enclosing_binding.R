# no-diag
# `<<-` from a nested closure is a possible type update to the enclosing
# binding (issue #374), so the initialized-to-NULL json-parser shape and
# its relatives stay silent: the writes have run before the conditions
# evaluate at runtime, and the checker cannot prove otherwise without
# call-graph evidence.
token <- NULL
read <- function(value) token <<- value
read(TRUE)
while (token) {
  token <<- FALSE
}

json <- local({
  ptr <- 1
  peek <- function() tokens[ptr] <<- 'EOF'
  parse_value <- function() if (tokens[ptr] != '}') peek()
})

state <- list(done = FALSE)
finish <- function() state$done <<- TRUE
finish()
if (state$done) 1L
