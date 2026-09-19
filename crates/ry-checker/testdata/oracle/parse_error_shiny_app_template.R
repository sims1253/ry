# oracle: must-flag-only RY000
# Byte-for-byte copy of the shiny corpus package's
# inst/app_template/app.R at ca180047 (the posit corpus pin), prepended
# with these marker lines only. The raw `{{ }}` whiskers blocks are R
# syntax errors (Rscript: "unexpected symbol"), so whole-file
# suppression (#467) must leave the three RY000s as the file's only
# diagnostics (#470).
ui <- fluidPage(
{{
# These blocks of code are processed with htmlTemplate()
if (isTRUE(module)) {
'  # ======== Modules ========
  # exampleModuleUI is defined in R/example-module.R
  wellPanel(
    h2("Modules example"),
    exampleModuleUI("examplemodule1", "Click counter #1"),
    exampleModuleUI("examplemodule2", "Click counter #2")
  ),
  # =========================
'
}
}}
  wellPanel(
    h2("Sorting example"),
    sliderInput("size", "Data size", min = 5, max = 20, value = 10),
{{
if (isTRUE(rdir)) {
  '    div("Lexically sorted sequence:"),'
} else {
  '    div("Sorted sequence:"),'
}
}}
    verbatimTextOutput("sequence")
  )
)

server <- function(input, output, session) {
{{
if (isTRUE(module)) {
'  # ======== Modules ========
  # exampleModuleServer is defined in R/example-module.R
  exampleModuleServer("examplemodule1")
  exampleModuleServer("examplemodule2")
  # =========================
'
}
}}
  data <- reactive({
{{
if (isTRUE(rdir)) {
'    # lexical_sort from R/example.R
    lexical_sort(seq_len(input$size))'
} else {
'    sort(seq_len(input$size))'
}
}}
  })
  output$sequence <- renderText({
    paste(data(), collapse = " ")
  })
}

shinyApp(ui, server)
