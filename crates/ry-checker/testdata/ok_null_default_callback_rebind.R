# no-diag
# A NULL callback default can be replaced with a local function and
# called later in the same scope.
render_nodes <- function(node_label = NULL) {
  if (is.null(node_label)) {
    node_label <- function(node) node$id
  }
  node_label(list(id = "a"))
}
