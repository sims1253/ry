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
}
