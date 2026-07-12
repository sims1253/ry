use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use ry_checker::{Checker, Project};
use ry_core::{RParser, SourceFile};

fn glue_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/vendor/glue/R");
    let mut paths: Vec<PathBuf> = fs::read_dir(root)
        .expect("read vendored glue sources")
        .map(|entry| entry.expect("read vendored glue entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("R"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .expect("glue source is inside the crate")
                .to_string_lossy()
                .into_owned();
            let source = fs::read_to_string(path).expect("read vendored glue source");
            (relative, source)
        })
        .collect()
}

fn parse_sources(sources: &[(String, String)]) -> Vec<(String, SourceFile)> {
    let mut parser = RParser::new().expect("initialize R parser");
    sources
        .iter()
        .map(|(path, source)| {
            let file = parser
                .parse(path, source)
                .expect("parse vendored glue source");
            (path.clone(), file)
        })
        .collect()
}

fn synthetic_source() -> String {
    (0..20_000)
        .map(|i| format!("x{i} <- c({i}, {}) * 2", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_large(c: &mut Criterion) {
    let source = glue_sources()
        .into_iter()
        .map(|(_, source)| source)
        .collect::<Vec<_>>()
        .join("\n");
    let mut parser = RParser::new().expect("initialize R parser");
    c.bench_function("parse_large", |b| {
        b.iter(|| {
            parser
                .parse("glue-all.R", black_box(&source))
                .expect("parse glue")
        });
    });
}

fn check_project_glue(c: &mut Criterion) {
    let parsed = parse_sources(&glue_sources());
    let mut project = Project::new();
    for (path, file) in parsed {
        project.add_file(path, file);
    }
    c.bench_function("check_project_glue", |b| {
        b.iter(|| black_box(project.check()));
    });
}

fn check_single_synthetic(c: &mut Criterion) {
    let source = synthetic_source();
    let mut parser = RParser::new().expect("initialize R parser");
    let file = parser
        .parse("synthetic.R", &source)
        .expect("parse synthetic source");
    c.bench_function("check_single_synthetic", |b| {
        b.iter(|| {
            let mut checker = Checker::new("synthetic.R");
            black_box(checker.check(black_box(&file)));
        });
    });
}

fn lsp_edit_sim(c: &mut Criterion) {
    let sources = glue_sources();
    let parsed: Vec<(String, std::sync::Arc<SourceFile>)> = parse_sources(&sources)
        .into_iter()
        .map(|(path, file)| (path, std::sync::Arc::new(file)))
        .collect();
    let edited_index = sources
        .iter()
        .position(|(path, _)| path.ends_with("glue.R"))
        .unwrap_or(0);
    let (edited_path, original) = &sources[edited_index];
    let edited_sources = [
        format!("{original}\n.ry_bench_value <- 1L\n"),
        format!("{original}\n.ry_bench_value <- 2L\n"),
    ];
    let mut parser = RParser::new().expect("initialize R parser");
    let mut edit = 0usize;
    let mut project = Project::new();
    for (path, file) in &parsed {
        project.add_file(path.clone(), file.as_ref().clone());
    }
    black_box(project.check_incremental());

    c.bench_function("lsp_edit_sim", |b| {
        b.iter(|| {
            edit ^= 1;
            let changed = std::sync::Arc::new(
                parser
                    .parse(edited_path, black_box(&edited_sources[edit]))
                    .expect("reparse edited glue source"),
            );
            // Model the LSP's all-document cache snapshot: unchanged files
            // clone only Arc handles, while the changed AST is forwarded to
            // the persistent Project and deep-cloned once into its cache.
            let cached_files: Vec<_> = parsed
                .iter()
                .map(|(path, file)| {
                    (
                        path.clone(),
                        if path == edited_path {
                            std::sync::Arc::clone(&changed)
                        } else {
                            std::sync::Arc::clone(file)
                        },
                    )
                })
                .collect();
            black_box(cached_files);
            project.update_file(edited_path.clone(), changed.as_ref().clone());
            black_box(project.check_incremental());
        });
    });
}

criterion_group! {
    name = performance;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = parse_large, check_project_glue, check_single_synthetic, lsp_edit_sim
}
criterion_main!(performance);
