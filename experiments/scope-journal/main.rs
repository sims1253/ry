use std::{path::Path, time::Instant};
fn paths(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_symlink() && p.is_dir() {
            continue;
        }
        if p.is_dir() {
            paths(&p, out)
        } else if p.extension().is_some_and(|x| x == "R") {
            out.push(p)
        }
    }
}
fn main() {
    let mut files = Vec::new();
    paths(Path::new(&std::env::args().nth(1).unwrap()), &mut files);
    files.sort();
    let mut parser = ry_core::RParser::new().unwrap();
    let parsed: Vec<_> = files
        .iter()
        .map(|f| {
            parser
                .parse(f.to_str().unwrap(), &std::fs::read_to_string(f).unwrap())
                .unwrap()
        })
        .collect();
    let start = Instant::now();
    let mut diagnostics = Vec::new();
    for file in &parsed {
        let mut checker = ry_checker::Checker::new(&file.path);
        checker.check(file);
        diagnostics.extend(checker.take_diagnostics());
    }
    println!(
        "files={} diagnostics={} micros={}",
        files.len(),
        diagnostics.len(),
        start.elapsed().as_micros()
    );
    if let Some(output) = std::env::args().nth(2) {
        std::fs::write(output, format!("{diagnostics:?}")).unwrap();
    }
}
