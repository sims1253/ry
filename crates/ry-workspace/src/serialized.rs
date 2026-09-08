//! Enumerate serialized R bindings without evaluating R code.

use super::file_stem_binding;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

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
    /// What the cached inventory was derived from. A mismatch on any field
    /// means the entry is stale. `cap` is part of it because raising
    /// `max-serialized-bytes` must re-enumerate a file that was previously
    /// reduced to its stem.
    type Stamp = (u64, u128, u64);
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<HashMap<PathBuf, (Stamp, SerializedInventory)>>,
    > = std::sync::OnceLock::new();
    let Ok(metadata) = std::fs::metadata(path) else {
        return SerializedInventory {
            bindings: HashSet::new(),
            degraded: false,
        };
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let stamp: Stamp = (metadata.len(), modified, cap);
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    // A panic in another thread poisons this mutex. The cache guards no
    // invariant, so recover the map instead of cascading the panic into
    // the LSP.
    if let Some(inventory) = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(path)
        .filter(|(cached, _)| *cached == stamp)
        .map(|(_, inventory)| inventory.clone())
    {
        return inventory;
    }
    let inventory = serialized_inventory_uncached(path, cap);
    // Keyed on the path, not on (path, stamp): a long-lived LSP session
    // re-checks the same data files after every edit, and keying on the
    // stamp would retain one binding set per historical version forever.
    // Replacing the entry bounds the cache by the number of distinct files.
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(path.to_path_buf(), (stamp, inventory.clone()));
    inventory
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
    /// temporary package root.
    fn gzipped_sysdata(bindings: &[(&str, ProbeValue)]) -> PathBuf {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.keep();
        std::fs::create_dir_all(root.join("R")).expect("mkdir R");
        let path = root.join("R/sysdata.rda");
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(&rda_v2_workspace(bindings))
            .expect("encode workspace");
        std::fs::write(&path, encoder.finish().expect("finish gzip")).expect("write sysdata");
        path
    }

    #[test]
    fn multi_mib_workspace_enumerates_under_the_default_cap() {
        // gt ships an 8 MB decoded R/sysdata.rda; a default cap below real
        // sysdata sizes degrades every such package to a file-stem binding
        // and re-flags all internal lookup tables as unbound (#378).
        let cap = ry_config::Config::default().max_serialized_bytes;
        let path = gzipped_sysdata(&[
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
        let path = gzipped_sysdata(&[("locales", ProbeValue::Doubles(4))]);
        let inventory = serialized_inventory(&path, 64);
        assert!(inventory.degraded);
        assert_eq!(inventory.bindings, HashSet::from(["sysdata".to_string()]));
    }

    #[test]
    fn workspace_over_the_default_cap_stays_bounded_and_degraded() {
        // Above the default the reader still stops at cap + 1 decoded bytes:
        // enumeration is skipped, the stem fallback keeps RY010 alive, and
        // the degraded flag drives the user-visible note.
        let cap = ry_config::Config::default().max_serialized_bytes;
        let path = gzipped_sysdata(&[
            ("a", ProbeValue::Doubles(1_100_000)),
            ("b", ProbeValue::Doubles(1_100_000)),
        ]);
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
    fn inventory_names_are_existence_only_across_value_kinds() {
        // sysdata workspaces hold tables, options, and lookups alike; a name
        // may equally hold a function. The inventory returns names only, so
        // serialized files never lend kind or type certainty to analysis.
        let path = gzipped_sysdata(&[
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
