use super::*;

/// Statement blocks walked while checking `src`.
fn block_walks(src: &str) -> usize {
    let mut checker = Checker::new("test.R");
    checker.check(&parse_file("test.R", src));
    checker.block_walks
}

type Shape = fn(usize) -> String;

/// #893: each nested function literal used to be walked once per walk of
/// its parent, and several times when discarding, so the work grew by 2x
/// to 5x per level. Now each level adds the same number of block walks.
/// The check compares the growth over two equal depth steps, so it holds
/// for linear work and fails for quadratic or exponential work, without
/// timing anything.
#[test]
fn nested_closure_work_grows_linearly_with_depth() {
    let shapes: [(&str, Shape, usize); 7] = [
        (
            "assigned",
            |n| format!("{}1{}", "f <- function() { ".repeat(n), " }".repeat(n)),
            4,
        ),
        (
            "bare statement",
            |n| format!("{}1{}", "function() { ".repeat(n), " }".repeat(n)),
            4,
        ),
        (
            "assigned with formals",
            |n| {
                let mut source = String::new();
                for level in 0..n {
                    source.push_str(&format!("f{level} <- function(x{level}) {{ "));
                }
                source + "x0" + &" }".repeat(n)
            },
            4,
        ),
        (
            "lapply",
            |n| {
                format!(
                    "{}1{}",
                    "lapply(x, function(i) { ".repeat(n),
                    " })".repeat(n)
                )
            },
            2,
        ),
        (
            "vapply",
            |n| {
                format!(
                    "{}1{}",
                    "vapply(x, function(i) { ".repeat(n),
                    " }, 1)".repeat(n)
                )
            },
            2,
        ),
        (
            "Reduce",
            |n| {
                format!(
                    "{}1{}",
                    "Reduce(function(a, b) { ".repeat(n),
                    " }, x)".repeat(n)
                )
            },
            3,
        ),
        (
            "assigned around lapply",
            |n| {
                format!(
                    "{}1{}",
                    "f <- function(x) { lapply(x, function(i) { ".repeat(n),
                    " }) }".repeat(n)
                )
            },
            2,
        ),
    ];
    for (name, shape, step) in shapes {
        let walks = [1, 2, 3].map(|k| block_walks(&shape(k * step)));
        let (first, second) = (walks[1] - walks[0], walks[2] - walks[1]);
        assert!(
            first > 0,
            "{name}: {walks:?} block walks do not grow with depth"
        );
        assert!(
            2 * second <= 3 * first,
            "{name}: {walks:?} block walks at depths {step}, {}, {}",
            2 * step,
            3 * step
        );
    }
}

/// The fourth nested callback hits the closure-depth cap, but its
/// constant body still types it: `v` is character, so `v + 1L` errors in
/// R ("non-numeric argument to binary operator"), as main reports.
#[test]
fn nested_sapply_callbacks_keep_their_return_type() {
    let diags = check(
        "v <- sapply(1L, function(a) sapply(1L, function(b) sapply(1L, function(c) sapply(1L, function(d) \"x\"))))\n\
         v + 1L\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from character + integer, got {diags:?}"
    );
}
