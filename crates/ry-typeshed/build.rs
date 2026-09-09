//! Build-time compression of the vendored typeshed stubs.
//!
//! The JSON under `vendor/` is the source of truth and stays
//! uncompressed in the repository for the sync tooling and review. This
//! script deflates every stub into `OUT_DIR`, and the library embeds
//! those blobs with `include_bytes!` instead of the raw JSON, which
//! cuts most of the embedded data from the binary. Decompression stays
//! lazy in `src/lib.rs`, at the original parse points.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::DeflateEncoder;

/// Recursively list the vendored stub JSON, sorted so the output is
/// deterministic across machines.
fn stub_paths(vendor: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => panic!("read {}: {error}", dir.display()),
        };
        for entry in entries {
            let path = entry
                .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
                .path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                out.push(path);
            }
        }
    }
    let mut paths = Vec::new();
    walk(vendor, &mut paths);
    paths.sort();
    paths
}

/// Deflate `data` into the raw stream format (no gzip/zlib wrapper):
/// smallest payload, and the producer/consumer are both this crate.
fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(data).expect("deflate vendored stub");
    encoder.finish().expect("finish deflate stream")
}

fn main() {
    let vendor = Path::new("vendor");
    // A directory argument makes cargo watch the whole tree, so a
    // typeshed sync re-runs this script on the next build.
    println!("cargo:rerun-if-changed={}", vendor.display());
    let out_dir =
        PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR for build scripts"));

    let mut stems: Vec<String> = Vec::new();
    for path in stub_paths(vendor) {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| panic!("vendored stub path must be UTF-8: {}", path.display()));
        assert!(
            !stems.contains(&stem),
            "vendored stubs must have unique file stems; duplicate: {stem}"
        );

        let json =
            fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        fs::write(out_dir.join(format!("{stem}.json.deflate")), deflate(&json))
            .unwrap_or_else(|error| panic!("write {stem}.json.deflate: {error}"));
        stems.push(stem);
    }
    assert!(
        !stems.is_empty(),
        "no vendored stubs found under {} (run scripts/sync_typeshed.sh)",
        vendor.display()
    );

    // Cargo does not clean OUT_DIR between build-script reruns, so a
    // renamed or removed vendor file would leave its stale blob behind.
    // Delete any deflate output that no longer corresponds to a vendor
    // stub; everything left over is exactly the set just written.
    for entry in fs::read_dir(&out_dir).expect("read OUT_DIR") {
        let path = entry.expect("read OUT_DIR entry").path();
        let stale = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json.deflate"))
            .map(|stem| !stems.contains(&stem.to_owned()))
            .unwrap_or(false);
        if stale {
            fs::remove_file(&path)
                .unwrap_or_else(|error| panic!("remove stale {}: {error}", path.display()));
        }
    }
}
