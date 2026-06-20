//! Real-world baseline test.
//!
//! Runs the checker against a small set of vendored "realistic" R snippets
//! (each modeled after real CRAN/base R code patterns) and *reports* the
//! diagnostic distribution without asserting it. This catches large
//! regressions in false-positive rate when reviewed, but does not break
//! the build when the distribution shifts.
//!
//! Use `cargo test real_world -- --nocapture` to see the table.

use std::collections::BTreeMap;

use ry_checker::Checker;
use ry_core::RParser;

/// Snippets modeled on patterns observed in real R packages. None of
/// these are *expected* to be silent; the goal is to track what we
/// currently emit so shifts are visible in code review.
const SNIPPETS: &[(&str, &str)] = &[
    (
        "base_mean_default",
        r#"mean <- function(x, trim = 0, na.rm = FALSE) {
  if (!na.rm && any(is.na(x))) return(NA_real_)
  sum(x) / length(x)
}
"#,
    ),
    (
        "tidyverse_pipe",
        r#"library(magrittr)
result <- mtcars %>%
  subset(cyl == 4) %>%
  subset(mpg > 25)
"#,
    ),
    (
        "s3_dispatch",
        r#"print.myclass <- function(x, ...) {
  cat("My class:", x$name, "\n")
  invisible(x)
}
x <- structure(list(name = "foo"), class = "myclass")
print(x)
"#,
    ),
    (
        "na_propagation",
        r#"x <- c(1, 2, NA, 4)
y <- x + 10
z <- mean(x, na.rm = TRUE)
"#,
    ),
    (
        "coercion_ladder",
        r#"x <- c(1L, 2L, 3L)
y <- c(x, 4.5)
z <- c(y, "end")
w <- if (length(z) > 0) z else NA
"#,
    ),
    (
        "loop_with_accumulator",
        r#"total <- 0
for (i in 1:100) {
  if (i %% 2 == 0) {
    total <- total + i
  }
}
"#,
    ),
    (
        "vectorized_op",
        r#"x <- 1:10
y <- x * 2 + 1
ok <- y > 5
sel <- which(ok)
"#,
    ),
];

fn run(name: &str, src: &str) -> Vec<String> {
    let mut parser = match RParser::new() {
        Ok(p) => p,
        Err(e) => {
            println!("{}: parser init failed: {}", name, e);
            return Vec::new();
        }
    };
    let file = match parser.parse(name, src) {
        Ok(f) => f,
        Err(e) => {
            println!("{}: parse failed: {}", name, e);
            return Vec::new();
        }
    };
    let mut c = Checker::new(name);
    c.check(&file);
    c.take_diagnostics()
        .into_iter()
        .map(|d| d.code.to_string())
        .collect()
}

#[test]
fn real_world_distribution() {
    let mut grand_total: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_snippet: Vec<(&str, BTreeMap<String, usize>, usize)> = Vec::new();

    for (name, src) in SNIPPETS {
        let codes = run(name, src);
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for c in &codes {
            *counts.entry(c.clone()).or_insert(0) += 1;
            *grand_total.entry(c.clone()).or_insert(0) += 1;
        }
        per_snippet.push((name, counts.clone(), codes.len()));
    }

    println!("\n=== ry real-world baseline ===\n");
    println!("{:<24} {:<8} codes", "snippet", "diags");
    println!("{}", "-".repeat(64));
    for (name, counts, n) in &per_snippet {
        let codes_str = if counts.is_empty() {
            "(clean)".to_string()
        } else {
            counts
                .iter()
                .map(|(k, v)| format!("{}x{}", v, k))
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!("{:<24} {:<8} {}", name, n, codes_str);
    }
    println!("{}", "-".repeat(64));
    let total: usize = per_snippet.iter().map(|(_, _, n)| *n).sum();
    println!(
        "{:<24} {:<8} {}",
        "TOTAL",
        total,
        grand_total
            .iter()
            .map(|(k, v)| format!("{}x{}", v, k))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "\nThis is a baseline report, not an assertion. If the numbers shift\n\
         significantly between commits, investigate false-positive regressions.\n"
    );
}
