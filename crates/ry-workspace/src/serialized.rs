//! Enumerate serialized R bindings without evaluating R code.

use super::file_stem_binding;
use std::collections::HashSet;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Inventory of a serialized R data file (`.rda`/`.rdata`). `bindings`
/// are the enumerated object names, or a single file-stem fallback when
/// the decoded payload exceeds the byte cap. `degraded` is true when
/// enumeration was skipped, which means the binding set is an
/// approximation rather than the real object names.
#[derive(Clone)]
pub(super) struct SerializedInventory {
    pub(super) bindings: HashSet<String>,
    pub(super) degraded: bool,
}

/// Read only the top-level tags from an R serialization stream. `.rda`
/// workspaces are serialized pairlists whose tags are the binding names. The
/// parser's lazy mode skips vector payload allocation, and bzip2 streams are
/// decompressed in-process; no R runtime or project code is executed.
///
/// Returns the enumerated object names plus a `degraded` flag. When the
/// decoded payload exceeds the byte cap, enumeration is skipped and the
/// binding set is reduced to a single file-stem fallback ([`file_stem_binding`])
/// so unbound-variable analysis (RY010) stays live.
pub(super) fn serialized_inventory(path: &Path, cap: u64) -> SerializedInventory {
    let mut inventory =
        cached_inventory(path, cap).unwrap_or_else(|| serialized_inventory_uncached(path, cap));
    if inventory.degraded {
        inventory.bindings = file_stem_binding(path);
    }
    inventory
}

fn cached_inventory(path: &Path, cap: u64) -> Option<SerializedInventory> {
    // Without a change-time stamp, re-read rather than trust a preserved mtime.
    #[cfg(not(unix))]
    {
        let _ = (path, cap);
        None
    }
    #[cfg(unix)]
    {
        const MAX_CACHED_INVENTORIES: usize = 1024;
        type Stamp = (u64, i64, i64, u64, u64, u64);
        use std::{collections::VecDeque, path::PathBuf};
        type Entry = (PathBuf, Stamp, SerializedInventory);
        static CACHE: std::sync::OnceLock<std::sync::Mutex<VecDeque<Entry>>> =
            std::sync::OnceLock::new();
        let key = path.canonicalize().ok()?;
        let metadata = std::fs::metadata(&key).ok()?;
        let stamp = (
            metadata.len(),
            metadata.ctime(),
            metadata.ctime_nsec(),
            metadata.dev(),
            metadata.ino(),
            cap,
        );
        let cache = CACHE.get_or_init(Default::default);
        let lock = || {
            cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        };
        {
            let mut entries = lock();
            if let Some(index) = entries.iter().position(|(path, _, _)| path == &key) {
                let entry = entries.remove(index)?;
                if entry.1 == stamp {
                    let inventory = entry.2.clone();
                    entries.push_back(entry);
                    return Some(inventory);
                }
            }
        }
        let inventory = serialized_inventory_uncached(&key, cap);
        let mut entries = lock();
        entries.retain(|(path, _, _)| path != &key);
        if entries.len() == MAX_CACHED_INVENTORIES {
            entries.pop_front();
        }
        entries.push_back((key, stamp, inventory.clone()));
        Some(inventory)
    }
}

fn serialized_inventory_uncached(path: &Path, cap: u64) -> SerializedInventory {
    // Decoded payload exceeded the byte cap: fall back to the file stem
    // as a single conservative binding. Callers flag the scope as
    // degraded so the user knows RY010 precision dropped for that file.
    let degraded = |path: &Path| SerializedInventory {
        bindings: file_stem_binding(path),
        degraded: true,
    };
    let empty = || SerializedInventory {
        bindings: HashSet::new(),
        degraded: false,
    };

    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return empty(),
    };
    let bytes = match read_serialized(file, cap) {
        Decoded::Bytes(bytes) => bytes,
        Decoded::OverCap => return degraded(path),
        Decoded::Failed => return empty(),
    };
    let payload = bytes
        .strip_prefix(b"RDX2\n")
        .or_else(|| bytes.strip_prefix(b"RDX3\n"))
        .unwrap_or(&bytes);
    let Ok(parsed) = rds2rust::read_rds_lazy(payload) else {
        return empty();
    };
    let bindings = match parsed.object.into_concrete() {
        rds2rust::RObject::Pairlist(elements) => elements
            .into_iter()
            .filter_map(|element| element.tag.map(|tag| tag.to_string()))
            .collect(),
        _ => HashSet::new(),
    };
    SerializedInventory {
        bindings,
        degraded: false,
    }
}

fn read_serialized(reader: impl std::io::Read, cap: u64) -> Decoded {
    let mut reader = reader;
    let mut prefix = Vec::with_capacity(6);
    if reader.by_ref().take(6).read_to_end(&mut prefix).is_err() {
        return Decoded::Failed;
    }
    let reader = prefix.as_slice().chain(reader);
    if prefix.starts_with(b"BZh") {
        decode_capped(bzip2::read::BzDecoder::new(reader), cap)
    } else if prefix.starts_with(&[0x1f, 0x8b]) {
        decode_capped(flate2::read::GzDecoder::new(reader), cap)
    } else if prefix.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]) {
        decode_capped(liblzma::read::XzDecoder::new(reader), cap)
    } else {
        decode_capped(reader, cap)
    }
}

/// Outcome of decoding a compressed serialization under the byte cap.
enum Decoded {
    /// Payload decoded and fits within the cap.
    Bytes(Vec<u8>),
    /// Payload exceeds the cap.
    OverCap,
    /// Stream is unreadable or corrupt.
    Failed,
}

/// Decode `decoder` fully, stopping once the payload provably exceeds
/// `cap`. Reading at most `cap + 1` bytes distinguishes an
/// exact-cap payload from a larger one without decoding all of it.
fn decode_capped(decoder: impl std::io::Read, cap: u64) -> Decoded {
    let mut decoded = Vec::new();
    if decoder
        .take(cap.saturating_add(1))
        .read_to_end(&mut decoded)
        .is_err()
    {
        return Decoded::Failed;
    }
    if decoded.len() as u64 > cap {
        return Decoded::OverCap;
    }
    Decoded::Bytes(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    struct TinyReads<'a> {
        bytes: &'a [u8],
        consumed: usize,
    }
    impl Read for TinyReads<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let length = buffer.len().min(1);
            let count = self.bytes.read(&mut buffer[..length])?;
            self.consumed += count;
            Ok(count)
        }
    }

    #[test]
    fn serialization_reads_are_capped_and_detect_fragmented_headers() {
        let payload: Vec<u8> = (0..=255).collect();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), Default::default());
        gzip.write_all(&payload).unwrap();
        let mut bzip = bzip2::write::BzEncoder::new(Vec::new(), Default::default());
        bzip.write_all(&payload).unwrap();
        let mut xz = liblzma::write::XzEncoder::new(Vec::new(), 6);
        xz.write_all(&payload).unwrap();
        for bytes in [
            payload.clone(),
            gzip.finish().unwrap(),
            bzip.finish().unwrap(),
            xz.finish().unwrap(),
        ] {
            let mut reader = TinyReads {
                bytes: &bytes,
                consumed: 0,
            };
            assert!(
                matches!(read_serialized(&mut reader, 256), Decoded::Bytes(result) if result == payload)
            );
            assert!(matches!(
                read_serialized(bytes.as_slice(), 255),
                Decoded::OverCap
            ));
        }
        let bytes = vec![b'x'; 65536];
        let mut reader = TinyReads {
            bytes: &bytes,
            consumed: 0,
        };
        assert!(matches!(read_serialized(&mut reader, 16), Decoded::OverCap));
        assert_eq!(reader.consumed, 17);
        for corrupt in [
            b"BZh".as_slice(),
            &[0x1f, 0x8b],
            &[0xfd, b'7', b'z', b'X', b'Z', 0],
        ] {
            assert!(matches!(read_serialized(corrupt, 256), Decoded::Failed));
        }
    }

    /// Value kinds the probe workspace writer can emit. Doubles and logicals
    /// are enough to pin that inventory results never depend on the value.
    #[derive(Clone, Copy)]
    enum ProbeValue {
        Doubles(usize),
        Logicals(usize),
    }

    /// Write an uncompressed R v2 `.rda` workspace whose top-level pairlist
    /// binds each name to a zero-filled vector. The byte grammar mirrors what
    /// R's `save(..., version = 2, compress = FALSE)` emits for tagged cells:
    /// per cell the `LISTSXP` flags with the tag bit, a `SYMSXP` tag whose
    /// print name is a UTF-8 `CHARSXP`, the value item, and a final
    /// `NILVALUE_SXP` terminator. Zero payloads keep the stream compressible
    /// so a gzip-wrapped copy stays tiny on disk while its decoded size
    /// crosses the cap under test.
    fn rda_v2_workspace(bindings: &[(&str, ProbeValue)]) -> Vec<u8> {
        fn u32_be(out: &mut Vec<u8>, value: u32) {
            out.extend_from_slice(&value.to_be_bytes());
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"RDX2\nX\n");
        u32_be(&mut out, 2); // serialization format version
        u32_be(&mut out, 0x0004_0601); // writer version (any plausible R works)
        u32_be(&mut out, 0x0002_0300); // minimum reader version
        for (name, value) in bindings {
            u32_be(&mut out, 0x0402); // LISTSXP cell with tag
            u32_be(&mut out, 1); // SYMSXP tag item
            u32_be(&mut out, 0x40009); // UTF-8 CHARSXP print name
            u32_be(&mut out, name.len() as u32);
            out.extend_from_slice(name.as_bytes());
            match *value {
                ProbeValue::Doubles(count) => {
                    u32_be(&mut out, 14); // REALSXP
                    u32_be(&mut out, count as u32);
                    out.resize(out.len() + count * 8, 0);
                }
                ProbeValue::Logicals(count) => {
                    u32_be(&mut out, 10); // LGLSXP
                    u32_be(&mut out, count as u32);
                    out.resize(out.len() + count * 4, 0);
                }
            }
        }
        u32_be(&mut out, 0xfe); // NILVALUE_SXP terminator
        out
    }

    /// Gzip the probe workspace and write it as `R/sysdata.rda` under a fresh
    /// temporary package root. The `TempDir` guard is returned so its lifetime
    /// covers the assertions; dropping it removes the fixture.
    fn gzipped_sysdata(bindings: &[(&str, ProbeValue)]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("R")).expect("mkdir R");
        let path = dir.path().join("R/sysdata.rda");
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(&rda_v2_workspace(bindings))
            .expect("encode workspace");
        std::fs::write(&path, encoder.finish().expect("finish gzip")).expect("write sysdata");
        (dir, path)
    }

    #[test]
    fn multi_mib_workspace_enumerates_under_the_default_cap() {
        // gt ships an 8 MB decoded R/sysdata.rda; a default cap below real
        // sysdata sizes degrades every such package to a file-stem binding
        // and re-flags all internal lookup tables as unbound (#378).
        let cap = ry_config::Config::default().max_serialized_bytes;
        let (_guard, path) = gzipped_sysdata(&[
            ("locales", ProbeValue::Doubles(200_000)),
            ("currencies", ProbeValue::Doubles(200_000)),
        ]);
        let inventory = serialized_inventory(&path, cap);
        assert!(!inventory.degraded, "3.2 MB workspace must enumerate");
        assert!(inventory.bindings.contains("locales"));
        assert!(inventory.bindings.contains("currencies"));
    }

    #[test]
    fn explicit_small_cap_still_degrades_to_the_file_stem() {
        // An explicit cap always wins over the raised default: users keep a
        // hard bound for adversarial or huge files.
        let (_guard, path) = gzipped_sysdata(&[("locales", ProbeValue::Doubles(4))]);
        let inventory = serialized_inventory(&path, 64);
        assert!(inventory.degraded);
        assert_eq!(inventory.bindings, HashSet::from(["sysdata".to_string()]));
    }

    #[test]
    fn workspace_marginally_over_the_default_cap_stays_bounded_and_degraded() {
        // A COMPLETE, VALID workspace whose decoded stream is only a few bytes
        // past the cap. The degraded stem-only outcome pins overflow
        // classification at the boundary: a stream just past the cap must
        // degrade exactly like a far larger one, never enumerate. It does
        // NOT by itself prove how many bytes were read — a full decode
        // followed by a length check would degrade identically — so the
        // consumption bound (stop after cap + 1 decoded bytes) is pinned
        // separately by the TinyReads assertions in
        // `serialization_reads_are_capped_...` above.
        let cap = ry_config::Config::default().max_serialized_bytes;
        // One binding of zero doubles: 19 header + 25 cell + 4 terminator
        // bytes; each extra double adds 8 bytes, so this count lands the
        // decoded stream a few bytes past the cap.
        let doubles = ((cap + 8 - 48) / 8) as usize;
        let stream = rda_v2_workspace(&[("a", ProbeValue::Doubles(doubles))]);
        assert!(
            stream.len() > cap as usize + 1,
            "fixture must decode to more than cap + 1 bytes"
        );
        assert!(
            (stream.len() - cap as usize) <= 16,
            "fixture should sit just past the cap, not far beyond it"
        );
        let (_guard, path) = gzipped_sysdata(&[("a", ProbeValue::Doubles(doubles))]);
        let inventory = serialized_inventory(&path, cap);
        assert!(inventory.degraded);
        assert_eq!(inventory.bindings, HashSet::from(["sysdata".to_string()]));
    }

    #[test]
    fn malformed_stream_yields_empty_inventory_without_degrading() {
        // Bytes that are not a serialization stream must not masquerade as an
        // over-cap file: the reader reports failure, not degradation, so the
        // scope keeps full RY010 precision instead of a stem fallback.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sysdata.rda");
        std::fs::write(&path, b"not a serialization stream").expect("write garbage");
        let inventory = serialized_inventory(&path, 1 << 20);
        assert!(!inventory.degraded);
        assert!(inventory.bindings.is_empty());
    }

    #[test]
    fn inventory_cache_tracks_caps_aliases_and_replacements() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.rda");
        let first = rda_v2_workspace(&[("first", ProbeValue::Doubles(1))]);
        let second = rda_v2_workspace(&[("other", ProbeValue::Doubles(1))]);
        assert_eq!(first.len(), second.len());
        std::fs::write(&path, &first).unwrap();
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(time)
            .unwrap();
        assert!(serialized_inventory(&path, 1).degraded);
        assert!(serialized_inventory(&path, 4096).bindings.contains("first"));
        let replacement = dir.path().join("new.rda");
        std::fs::write(&replacement, second).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_modified(time)
            .unwrap();
        std::fs::rename(replacement, &path).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), time);
        assert_eq!(
            serialized_inventory(&path, 4096).bindings,
            HashSet::from(["other".into()])
        );
        #[cfg(unix)]
        {
            let alias = dir.path().join("alias.rda");
            std::os::unix::fs::symlink(&path, &alias).unwrap();
            assert!(
                serialized_inventory(&alias, 4096)
                    .bindings
                    .contains("other")
            );
            assert_eq!(
                serialized_inventory(&alias, 1).bindings,
                HashSet::from(["alias".into()])
            );
            assert_eq!(
                serialized_inventory(&path, 1).bindings,
                HashSet::from(["data".into()])
            );
        }
    }

    #[test]
    fn inventory_names_are_existence_only_across_value_kinds() {
        // sysdata workspaces hold tables, options, and lookups alike; a name
        // may equally hold a function. The inventory returns names only, so
        // serialized files never lend kind or type certainty to analysis.
        let (_guard, path) = gzipped_sysdata(&[
            ("data_like", ProbeValue::Doubles(2)),
            ("flag_like", ProbeValue::Logicals(2)),
        ]);
        let inventory = serialized_inventory(&path, 1 << 20);
        assert_eq!(
            inventory.bindings,
            HashSet::from(["data_like".to_string(), "flag_like".to_string()])
        );
    }
}
