//! Compress registered stubs and generate their lazy-load table.

use std::collections::HashSet;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::DeflateEncoder;

fn stub_paths(dir: &Path, paths: &mut Vec<PathBuf>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in fs::read_dir(dir).unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
    {
        let path = entry.expect("read vendor entry").path();
        if path.is_dir() {
            stub_paths(&path, paths);
        } else if path.extension().is_some_and(|ext| ext == "json") {
            println!("cargo:rerun-if-changed={}", path.display());
            paths.push(path);
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=packages.txt");
    let packages: Vec<_> = include_str!("packages.txt").lines().collect();
    let registered: HashSet<_> = packages.iter().copied().chain(["base"]).collect();
    assert_eq!(
        registered.len(),
        packages.len() + 1,
        "duplicate package registration"
    );
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let mut paths = Vec::new();
    stub_paths(Path::new("vendor"), &mut paths);
    paths.sort();
    let mut stems = HashSet::new();
    for path in paths {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("UTF-8 stub name");
        assert!(
            stems.insert(stem.to_owned()),
            "duplicate vendored stub: {stem}"
        );
        if !registered.contains(stem) {
            println!(
                "cargo:warning=unregistered vendor stub {}; add it to packages.txt",
                path.display()
            );
            continue;
        }
        let json =
            fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&json).expect("deflate stub");
        fs::write(
            out_dir.join(format!("{stem}.json.deflate")),
            encoder.finish().expect("finish deflate"),
        )
        .expect("write compressed stub");
    }
    for name in &registered {
        assert!(
            stems.contains(*name),
            "missing registered vendor stub: {name}"
        );
    }
    // Cargo keeps OUT_DIR between builds, including outputs for removed stubs.
    for entry in fs::read_dir(&out_dir).expect("read OUT_DIR") {
        let path = entry.expect("read OUT_DIR entry").path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json.deflate"))
            .is_some_and(|stem| !registered.contains(stem))
        {
            fs::remove_file(path).expect("remove stale compressed stub");
        }
    }
    let mut specs = format!(
        "static PACKAGE_SPECS: [PackageSpec; {}] = [\n",
        packages.len()
    );
    for name in packages {
        let blob = out_dir.join(format!("{name}.json.deflate"));
        specs.push_str(&format!(
            "PackageSpec::new({name:?}, include_bytes!({blob:?})),\n"
        ));
    }
    specs.push_str("];\n");
    fs::write(out_dir.join("packages.rs"), specs).expect("write package table");
}
