//! Developer-only entry point for a bounded Project trace.
//! Run with `cargo run -p ry-checker --example project_trace -- --help`.

use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::sync::Arc;

use ry_checker::{Project, TraceOptions};
use ry_core::RParser;

fn run() -> Result<(), Box<dyn Error>> {
    let mut options = TraceOptions::default();
    let mut edit: Option<(String, String)> = None;
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => {
                println!(
                    "Usage: project_trace [--file-filter PATH] [--max-events N] [--max-event-bytes N] [--edit PATH REPLACEMENT.R] FILE.R [FILE.R ...]\n\nPaths are logical Project identities. --edit replaces one listed file with the UTF-8 contents of REPLACEMENT.R and records a second incremental check. Output is a JSON array of one or two trace reports; source contents are never included."
                );
                return Ok(());
            }
            "--file-filter" => options.file = Some(args.next().ok_or("missing file filter")?),
            "--max-events" => {
                options.max_events = args.next().ok_or("missing event limit")?.parse()?
            }
            "--max-event-bytes" => {
                options.max_event_bytes = args.next().ok_or("missing byte limit")?.parse()?
            }
            "--edit" => {
                if edit.is_some() {
                    return Err("--edit may be supplied once".into());
                }
                edit = Some((
                    args.next().ok_or("missing edited project path")?,
                    args.next().ok_or("missing replacement file")?,
                ));
            }
            "--" => files.extend(&mut args),
            option if option.starts_with('-') => {
                return Err(format!("unknown option: {option}").into());
            }
            path => files.push(path.to_owned()),
        }
    }
    if files.is_empty() {
        return Err("at least one R source file is required; see --help".into());
    }
    if files.iter().collect::<HashSet<_>>().len() != files.len() {
        return Err("duplicate project paths are not supported by this example".into());
    }
    if let Some((path, _)) = &edit
        && !files.contains(path)
    {
        return Err("--edit PATH must name one of the listed project files".into());
    }

    let mut parser = RParser::new()?;
    let mut project = Project::new();
    for path in files {
        let source = fs::read_to_string(&path)?;
        project.add_file(path.clone(), parser.parse(&path, &source)?);
    }
    project.enable_trace(options)?;
    let mut reports = Vec::new();
    project.check_incremental();
    reports.push(project.take_trace().expect("trace enabled"));
    if let Some((path, replacement)) = edit {
        let source = fs::read_to_string(replacement)?;
        project.update_file(path.clone(), Arc::new(parser.parse(&path, &source)?));
        project.check_incremental();
        reports.push(project.take_trace().expect("trace enabled"));
    }
    println!("{}", serde_json::to_string_pretty(&reports)?);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("project_trace: {error}");
        std::process::exit(1);
    }
}
