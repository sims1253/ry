//! `ry reduce`: shrink an R file while `ry check` still reports one
//! diagnostic, to turn a false positive found in a real project into a
//! small reproduction.
//!
//! Each candidate is checked by running this binary as
//! `ry check - --stdin-filename <file>`, so the file keeps its project,
//! configuration, and package context, and the file on disk is never
//! changed. A candidate is kept when it still reports a diagnostic with the
//! same code and message as the target. With `--rscript`, it must also run
//! without error under `Rscript --vanilla`, so the reduction stays a false
//! positive instead of turning into real broken code.
//!
//! The reduction is greedy over syntax: it first deletes runs of top-level
//! expressions, then deletes single expressions, call arguments, and
//! function parameters, then replaces an expression with one of its
//! children (`if (p) { x }` to `x`). It repeats until nothing is removed.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use miette::{IntoDiagnostic, Result, miette};

/// How long one `Rscript` run may take before the candidate is rejected.
/// Deleting a loop's update can make a candidate run forever.
const RSCRIPT_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct ReduceArgs {
    pub file: PathBuf,
    pub code: String,
    pub line: Option<usize>,
    pub rscript: bool,
    pub output: Option<PathBuf>,
}

pub(crate) fn run_reduce(args: ReduceArgs) -> Result<ExitCode> {
    let source = std::fs::read_to_string(&args.file)
        .map_err(|error| miette!("cannot read {}: {error}", args.file.display()))?;
    let code = args.code.to_ascii_uppercase();
    let exe = std::env::current_exe().into_diagnostic()?;
    let mut oracle = Oracle {
        exe,
        file: args.file.clone(),
        code,
        message: String::new(),
        rscript: args.rscript,
        checks: 0,
    };

    let found = oracle.diagnostics(&source)?;
    let target = found
        .iter()
        .filter(|(_, code, _)| *code == oracle.code)
        .find(|(line, _, _)| args.line.is_none_or(|wanted| *line == wanted));
    let Some((line, _, message)) = target else {
        let place = args
            .line
            .map_or(String::new(), |line| format!(" on line {line}"));
        eprintln!(
            "ry reduce: {} reports no {}{place}",
            args.file.display(),
            oracle.code
        );
        return Ok(ExitCode::FAILURE);
    };
    eprintln!(
        "ry reduce: keeping {} at line {line}: {message}",
        oracle.code
    );
    oracle.message = message.clone();
    if oracle.rscript && !oracle.runs_in_r(&source)? {
        eprintln!(
            "ry reduce: Rscript fails on {}; --rscript needs a file that runs",
            args.file.display()
        );
        return Ok(ExitCode::FAILURE);
    }

    let reduced = reduce(source.clone(), &mut |candidate| oracle.keeps(candidate))?;
    eprintln!(
        "ry reduce: {} to {} lines in {} checks",
        source.lines().count(),
        reduced.lines().count(),
        oracle.checks
    );
    match &args.output {
        Some(path) => std::fs::write(path, &reduced)
            .map_err(|error| miette!("cannot write {}: {error}", path.display()))?,
        None => print!("{reduced}"),
    }
    Ok(ExitCode::SUCCESS)
}

/// Decides whether a candidate still shows the target diagnostic.
struct Oracle {
    exe: PathBuf,
    file: PathBuf,
    code: String,
    message: String,
    rscript: bool,
    checks: usize,
}

impl Oracle {
    /// `(line, code, message)` for each diagnostic `ry check` reports on
    /// `source` read in place of the file.
    fn diagnostics(&mut self, source: &str) -> Result<Vec<(usize, String, String)>> {
        self.checks += 1;
        let mut child = Command::new(&self.exe)
            .args(["check", "-", "--stdin-filename"])
            .arg(&self.file)
            .args(["--output-format", "json", "--exit-zero"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .into_diagnostic()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let input = source.as_bytes().to_vec();
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let output = child.wait_with_output().into_diagnostic()?;
        // A check that stops before reading stdin reports its own error.
        let _ = writer.join();
        if !output.status.success() {
            return Err(miette!(
                "ry check failed on a candidate (exit {:?})",
                output.status.code()
            ));
        }
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).into_diagnostic()?;
        Ok(json
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| {
                (
                    d["line"].as_u64().unwrap_or(0) as usize,
                    d["code"].as_str().unwrap_or_default().to_string(),
                    d["message"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect())
    }

    fn keeps(&mut self, source: &str) -> Result<bool> {
        let kept = self
            .diagnostics(source)?
            .iter()
            .any(|(_, code, message)| *code == self.code && *message == self.message);
        Ok(kept && (!self.rscript || self.runs_in_r(source)?))
    }

    /// Whether `Rscript --vanilla` runs `source` to completion without
    /// error, in an empty temporary directory.
    fn runs_in_r(&mut self, source: &str) -> Result<bool> {
        let dir = tempfile::tempdir().into_diagnostic()?;
        let script = dir.path().join("reduce.R");
        std::fs::write(&script, source).into_diagnostic()?;
        let mut child = Command::new("Rscript")
            .arg("--vanilla")
            .arg(&script)
            .current_dir(dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| miette!("cannot run Rscript: {error}"))?;
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().into_diagnostic()? {
                return Ok(status.success());
            }
            if started.elapsed() > RSCRIPT_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// One edit to try: replace `start..end` with `replacement`.
struct Edit {
    start: usize,
    end: usize,
    replacement: String,
}

impl Edit {
    fn apply(&self, source: &str) -> String {
        format!(
            "{}{}{}",
            &source[..self.start],
            self.replacement,
            &source[self.end..]
        )
    }
}

/// Shrink `source` while `keeps` accepts it. `keeps` must accept `source`.
fn reduce(mut source: String, keeps: &mut dyn FnMut(&str) -> Result<bool>) -> Result<String> {
    loop {
        let before = source.len();
        source = delete_top_level_runs(source, keeps)?;
        for pass in [deletions as fn(&str) -> Vec<Edit>, hoists] {
            source = greedy(source, pass, keeps)?;
        }
        if source.len() == before {
            break;
        }
    }
    let tidy = tidy_blank_lines(&source);
    if tidy != source && keeps(&tidy)? {
        source = tidy;
    }
    Ok(source)
}

/// Apply the first accepted edit of `pass`, re-derive the edits from the
/// new source, and repeat until no edit is accepted.
fn greedy(
    mut source: String,
    pass: fn(&str) -> Vec<Edit>,
    keeps: &mut dyn FnMut(&str) -> Result<bool>,
) -> Result<String> {
    'restart: loop {
        for edit in pass(&source) {
            let candidate = edit.apply(&source);
            if candidate.len() < source.len() && keeps(&candidate)? {
                source = candidate;
                continue 'restart;
            }
        }
        return Ok(source);
    }
}

/// Delete runs of top-level expressions, halving the run length from half
/// the file down to one (the first stage of delta debugging).
fn delete_top_level_runs(
    mut source: String,
    keeps: &mut dyn FnMut(&str) -> Result<bool>,
) -> Result<String> {
    let mut run = top_level(&source).len() / 2;
    while run > 0 {
        let mut start = 0;
        loop {
            let spans = top_level(&source);
            if start >= spans.len() {
                break;
            }
            let last = (start + run).min(spans.len()) - 1;
            let edit = Edit {
                start: spans[start].0,
                end: spans[last].1,
                replacement: String::new(),
            };
            let candidate = edit.apply(&source);
            if keeps(&candidate)? {
                source = candidate;
            } else {
                start += run;
            }
        }
        run /= 2;
    }
    Ok(source)
}

fn parse(source: &str) -> Option<ry_core::Tree> {
    let mut parser = ry_core::RParser::new().ok()?;
    parser
        .parse_with_tree("reduce.R", source, None)
        .ok()
        .map(|(_, tree)| tree)
}

/// Byte ranges of the top-level expressions and comments.
fn top_level(source: &str) -> Vec<(usize, usize)> {
    let Some(tree) = parse(source) else {
        return Vec::new();
    };
    let root = tree.root_node();
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .map(|node| (node.start_byte(), node.end_byte()))
        .collect()
}

/// Every node, in document order.
fn nodes(tree: &ry_core::Tree) -> Vec<ry_core::Node<'_>> {
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        out.push(node);
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    out
}

/// Deleting a statement (an expression or comment in a file or a braced
/// block), a call argument, or a function parameter, largest first. A list
/// item takes one neighboring comma with it.
fn deletions(source: &str) -> Vec<Edit> {
    let Some(tree) = parse(source) else {
        return Vec::new();
    };
    let mut edits = Vec::new();
    for node in nodes(&tree) {
        let Some(parent) = node.parent() else {
            continue;
        };
        match parent.kind() {
            "program" | "braced_expression" if node.is_named() => edits.push(Edit {
                start: node.start_byte(),
                end: node.end_byte(),
                replacement: String::new(),
            }),
            "arguments" | "parameters" if matches!(node.kind(), "argument" | "parameter") => {
                let (start, end) = match (node.next_sibling(), node.prev_sibling()) {
                    (Some(next), _) if next.kind() == "comma" => {
                        let end = next
                            .next_sibling()
                            .filter(|after| after.kind() != ")")
                            .map_or(next.end_byte(), |after| after.start_byte());
                        (node.start_byte(), end)
                    }
                    (_, Some(prev)) if prev.kind() == "comma" => {
                        (prev.start_byte(), node.end_byte())
                    }
                    _ => (node.start_byte(), node.end_byte()),
                };
                edits.push(Edit {
                    start,
                    end,
                    replacement: String::new(),
                });
            }
            _ => {}
        }
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.end - edit.start));
    edits
}

/// Replacing an expression with one of its named children, largest
/// expression first: `if (p) { x }` with `{ x }`, `f(g(x))` with `g(x)`.
fn hoists(source: &str) -> Vec<Edit> {
    let Some(tree) = parse(source) else {
        return Vec::new();
    };
    let mut edits = Vec::new();
    for node in nodes(&tree) {
        if node.kind() == "program" || node.kind() == "comment" {
            continue;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "comment" || child.byte_range() == node.byte_range() {
                continue;
            }
            edits.push(Edit {
                start: node.start_byte(),
                end: node.end_byte(),
                replacement: source[child.byte_range()].to_string(),
            });
        }
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.end - edit.start));
    edits
}

/// Drop trailing spaces and blank lines; keep one final newline.
fn tidy_blank_lines(source: &str) -> String {
    let mut out = String::new();
    for line in source.lines() {
        let line = line.trim_end();
        if !line.is_empty() {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in oracle: the candidate still has a line adding a string.
    fn keeps_string_sum(candidate: &str) -> Result<bool> {
        Ok(candidate.contains("\"a\" + x") && candidate.contains("x <- 1"))
    }

    #[test]
    fn reduce_removes_unrelated_statements_arguments_and_wrappers() {
        let source = "# setup\nhelper <- function(a, b) a + b\nx <- 1\nif (TRUE) {\n  y <- \"a\" + x\n}\nhelper(1, 2)\n";
        let reduced = reduce(source.to_string(), &mut keeps_string_sum).unwrap();
        assert_eq!(reduced, "x <- 1\n\"a\" + x\n");
    }

    #[test]
    fn argument_deletion_keeps_the_call_well_formed() {
        let edits = deletions("f(a, b, c)\n");
        let candidates: Vec<String> = edits
            .iter()
            .map(|edit| edit.apply("f(a, b, c)\n"))
            .collect();
        for expected in ["f(b, c)\n", "f(a, c)\n", "f(a, b)\n"] {
            assert!(candidates.contains(&expected.to_string()), "{candidates:?}");
        }
    }
}
