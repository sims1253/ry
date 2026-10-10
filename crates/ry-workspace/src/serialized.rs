//! Enumerate serialized R bindings without evaluating R code.

use super::file_stem_binding;
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Inventory of a serialized R data file (`.rda`/`.rdata`). `bindings`
/// are the enumerated object names, or a conservative fallback when
/// enumeration fails. The status distinguishes a known empty workspace,
/// an absent path, and an inventory that could not be recovered.
#[derive(Clone)]
pub(super) struct SerializedInventory {
    pub(super) bindings: HashSet<String>,
    pub(super) status: InventoryStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InventoryStatus {
    Complete,
    /// The requested path did not exist when the single open was attempted.
    Missing,
    Unavailable(InventoryFailure),
}

impl InventoryStatus {
    /// The degradation to report; a missing file reads as a read failure.
    pub(super) fn failure(self) -> Option<InventoryFailure> {
        match self {
            Self::Complete => None,
            Self::Missing => Some(InventoryFailure::ReadFailure),
            Self::Unavailable(reason) => Some(reason),
        }
    }
}

/// Bounded cause of an unavailable serialized inventory. Adding a cause
/// requires choosing its user message and whether a file-stem fallback is
/// justified in `description` and `uses_file_stem` below.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum InventoryFailure {
    DecodedByteLimit,
    ParserResourceLimit,
    /// The decoded bytes reached the parser but did not form valid input.
    MalformedInput,
    UnsupportedInput,
    /// The serialized file could not be opened.
    ReadFailure,
    /// The opened file could not be read or decompressed.
    DecodeFailure,
    ParserFailure,
}

impl InventoryFailure {
    pub fn description(self) -> &'static str {
        match self {
            Self::DecodedByteLimit => "decoded-byte limit exceeded",
            Self::ParserResourceLimit => "serialized parser resource limit exceeded",
            Self::MalformedInput => "malformed serialized input",
            Self::UnsupportedInput => "unsupported serialized input",
            Self::ReadFailure => "serialized input could not be read",
            Self::DecodeFailure => "serialized input could not be decoded",
            Self::ParserFailure => "serialized parser failed (cause unavailable)",
        }
    }

    fn uses_file_stem(self) -> bool {
        matches!(self, Self::DecodedByteLimit | Self::ParserResourceLimit)
    }
}

/// Read only the top-level tags from an R serialization stream. `.rda`
/// workspaces are serialized pairlists whose tags are the binding names. The
/// parser's lazy mode skips vector payload allocation, and bzip2 streams are
/// decompressed in-process; no R runtime or project code is executed.
///
/// Resource limits use a single file-stem fallback ([`file_stem_binding`])
/// so unbound-variable analysis (RY010) stays live. Malformed, unsupported,
/// and unreadable inputs keep the existing empty-binding fallback, but are
/// explicitly reported as unavailable rather than known empty.
pub(super) fn serialized_inventory(path: &Path, cap: u64) -> SerializedInventory {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return SerializedInventory {
                bindings: HashSet::new(),
                status: InventoryStatus::Missing,
            };
        }
        Err(_) => {
            return SerializedInventory {
                bindings: HashSet::new(),
                status: InventoryStatus::Unavailable(InventoryFailure::ReadFailure),
            };
        }
    };
    let mut inventory = cached_inventory(path, cap, file);
    if matches!(inventory.status, InventoryStatus::Unavailable(reason) if reason.uses_file_stem()) {
        inventory.bindings = file_stem_binding(path);
    }
    inventory
}

fn cached_inventory(path: &Path, cap: u64, file: File) -> SerializedInventory {
    // Without a change-time stamp, re-read rather than trust a preserved mtime.
    #[cfg(not(unix))]
    {
        let _ = path;
        serialized_inventory_uncached(file, cap)
    }
    #[cfg(unix)]
    {
        const MAX_CACHED_INVENTORIES: usize = 1024;
        type Stamp = (u64, i64, i64, u64, u64, u64);
        use std::{collections::VecDeque, path::PathBuf};
        type Entry = (PathBuf, Stamp, SerializedInventory);
        static CACHE: std::sync::OnceLock<std::sync::Mutex<VecDeque<Entry>>> =
            std::sync::OnceLock::new();
        let Ok(key) = path.canonicalize() else {
            return serialized_inventory_uncached(file, cap);
        };
        let Ok(metadata) = file.metadata() else {
            return serialized_inventory_uncached(file, cap);
        };
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
                let entry = entries.remove(index).expect("located entry");
                if entry.1 == stamp {
                    let inventory = entry.2.clone();
                    entries.push_back(entry);
                    return inventory;
                }
            }
        }
        let inventory = serialized_inventory_uncached(file, cap);
        let mut entries = lock();
        entries.retain(|(path, _, _)| path != &key);
        if entries.len() == MAX_CACHED_INVENTORIES {
            entries.pop_front();
        }
        entries.push_back((key, stamp, inventory.clone()));
        inventory
    }
}

fn serialized_inventory_uncached(file: File, cap: u64) -> SerializedInventory {
    let unavailable = |reason| SerializedInventory {
        bindings: HashSet::new(),
        status: InventoryStatus::Unavailable(reason),
    };
    let bytes = match read_serialized(file, cap) {
        Decoded::Bytes(bytes) => bytes,
        Decoded::OverCap => return unavailable(InventoryFailure::DecodedByteLimit),
        Decoded::Failed => return unavailable(InventoryFailure::DecodeFailure),
    };
    let payload = bytes
        .strip_prefix(b"RDX2\n")
        .or_else(|| bytes.strip_prefix(b"RDX3\n"))
        .unwrap_or(&bytes);
    // R's A/B envelopes are valid serialization formats, but the reader
    // accepts XDR only. Classify the encoding from its explicit magic
    // before the upstream parser's generic InvalidFormat("Expected X") can
    // incorrectly call a valid ASCII workspace malformed.
    if [b"RDA2\n".as_slice(), b"RDA3\n", b"RDB2\n", b"RDB3\n"]
        .iter()
        .any(|magic| bytes.starts_with(magic))
        || payload.starts_with(b"A\n")
        || payload.starts_with(b"B\n")
    {
        return unavailable(InventoryFailure::UnsupportedInput);
    }
    let parsed = match rds2rust::read_rds_lazy(payload) {
        Ok(parsed) => parsed,
        Err(error) => return unavailable(classify_parser_error(&error)),
    };
    let bindings = match parsed.object.into_concrete() {
        rds2rust::RObject::Pairlist(elements) => elements
            .into_iter()
            .filter_map(|element| element.tag.map(|tag| tag.to_string()))
            .collect(),
        rds2rust::RObject::Null => HashSet::new(),
        _ => return unavailable(InventoryFailure::UnsupportedInput),
    };
    SerializedInventory {
        bindings,
        status: InventoryStatus::Complete,
    }
}

fn classify_parser_error(error: &rds2rust::Error) -> InventoryFailure {
    match error {
        rds2rust::Error::InvalidFormat(message)
            if message.starts_with("Parser nesting limit ")
                || message.starts_with("Materialized allocation of ")
                || message.starts_with("Allocation of ")
                    && message.contains(" bytes exceeds cap")
                || message.starts_with("Length ") && message.contains(" exceeds safe limit ")
                || message.starts_with("Allocation size overflow while parsing ")
                || message.starts_with("Length overflow while parsing ") =>
        {
            InventoryFailure::ParserResourceLimit
        }
        rds2rust::Error::MemoryBudgetExceeded { .. } => InventoryFailure::ParserResourceLimit,
        rds2rust::Error::UnsupportedVersion(_) | rds2rust::Error::Unsupported(_) => {
            InventoryFailure::UnsupportedInput
        }
        rds2rust::Error::InvalidFormat(_)
        | rds2rust::Error::UnexpectedEof
        | rds2rust::Error::UnexpectedEofDetail { .. }
        | rds2rust::Error::Utf8(_)
        | rds2rust::Error::InvalidReference(_)
        | rds2rust::Error::TruncatedLazyPayload { .. } => InventoryFailure::MalformedInput,
        _ => InventoryFailure::ParserFailure,
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
        assert_eq!(
            inventory.status,
            InventoryStatus::Complete,
            "3.2 MB workspace must enumerate"
        );
        assert!(inventory.bindings.contains("locales"));
        assert!(inventory.bindings.contains("currencies"));
    }

    #[test]
    fn explicit_small_cap_still_degrades_to_the_file_stem() {
        // An explicit cap always wins over the raised default: users keep a
        // hard bound for adversarial or huge files.
        let (_guard, path) = gzipped_sysdata(&[("locales", ProbeValue::Doubles(4))]);
        let inventory = serialized_inventory(&path, 64);
        assert_eq!(
            inventory.status,
            InventoryStatus::Unavailable(InventoryFailure::DecodedByteLimit)
        );
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
        assert_eq!(
            inventory.status,
            InventoryStatus::Unavailable(InventoryFailure::DecodedByteLimit)
        );
        assert_eq!(inventory.bindings, HashSet::from(["sysdata".to_string()]));
    }

    #[test]
    fn malformed_stream_is_reported_without_a_stem_fallback() {
        // Invalid bytes do not justify inventing the file-stem binding, but
        // must not be presented as a successfully inventoried empty file.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sysdata.rda");
        std::fs::write(&path, b"not a serialization stream").expect("write garbage");
        let inventory = serialized_inventory(&path, 1 << 20);
        assert_eq!(
            inventory.status,
            InventoryStatus::Unavailable(InventoryFailure::MalformedInput)
        );
        assert!(inventory.bindings.is_empty());
    }

    #[test]
    fn missing_path_is_distinct_from_empty_and_failed_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sysdata.rda");
        let inventory = serialized_inventory(&path, 4096);
        assert_eq!(inventory.status, InventoryStatus::Missing);
        assert!(inventory.bindings.is_empty());

        std::fs::write(
            &path,
            include_bytes!("../../../testdata/serialized/empty.rda"),
        )
        .unwrap();
        let inventory = serialized_inventory(&path, 4096);
        assert_eq!(inventory.status, InventoryStatus::Complete);
        assert!(inventory.bindings.is_empty());
    }

    #[test]
    fn parser_message_classifier_distinguishes_limits_from_truncation() {
        // These are the InvalidFormat message families in rds2rust 0.3.0.
        // The real nested-limit fixture separately proves one path through
        // the upstream parser. Constructed messages protect this classifier,
        // but cannot detect a future upstream reword on their own.
        let limits = [
            "Parser nesting limit 64 exceeded",
            "Materialized allocation of 1 bytes exceeds limits while parsing x",
            "Allocation of 1 bytes exceeds cap while parsing x",
            "Length 1 exceeds safe limit 1 while parsing x",
            "Allocation size overflow while parsing x",
            "Length overflow while parsing x",
        ];
        for message in limits {
            assert_eq!(
                classify_parser_error(&rds2rust::Error::InvalidFormat(message.into())),
                InventoryFailure::ParserResourceLimit,
                "{message}"
            );
        }
        for message in [
            "Length 1 (1 bytes) exceeds remaining 1 bytes while parsing x",
            "invalid R serialization header",
        ] {
            assert_eq!(
                classify_parser_error(&rds2rust::Error::InvalidFormat(message.into())),
                InventoryFailure::MalformedInput,
                "{message}"
            );
        }
        assert_eq!(
            classify_parser_error(&rds2rust::Error::MemoryBudgetExceeded {
                needed: 2,
                available: 1,
            }),
            InventoryFailure::ParserResourceLimit
        );
        assert_eq!(
            classify_parser_error(&rds2rust::Error::ParseError("opaque".into())),
            InventoryFailure::ParserFailure
        );
    }

    #[test]
    fn valid_ascii_workspace_is_unsupported_not_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sysdata.rda");
        let ascii = include_bytes!("../../../testdata/serialized/empty-ascii.rda");
        assert!(ascii.starts_with(b"RDA2\nA\n"));
        std::fs::write(&path, ascii).unwrap();
        let inventory = serialized_inventory(&path, 4096);
        assert_eq!(
            inventory.status,
            InventoryStatus::Unavailable(InventoryFailure::UnsupportedInput)
        );
        assert!(inventory.bindings.is_empty());
    }

    #[test]
    fn empty_workspace_and_parser_limit_have_distinct_inventory_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sysdata.rda");
        let cap = 4096;
        let empty = include_bytes!("../../../testdata/serialized/empty.rda");
        std::fs::write(&path, empty).unwrap();
        let inventory = serialized_inventory(&path, cap);
        assert_eq!(inventory.status, InventoryStatus::Complete);
        assert!(inventory.bindings.is_empty());

        let nested = include_bytes!("../../../testdata/serialized/nested-limit.rda");
        assert!(nested.len() < cap as usize);
        assert!(matches!(
            read_serialized(nested.as_slice(), cap),
            Decoded::Bytes(bytes) if bytes.len() < cap as usize
        ));
        std::fs::write(&path, nested).unwrap();
        let inventory = serialized_inventory(&path, cap);
        assert_eq!(
            inventory.status,
            InventoryStatus::Unavailable(InventoryFailure::ParserResourceLimit)
        );
        assert_eq!(inventory.bindings, HashSet::from(["sysdata".to_string()]));

        std::fs::write(&path, empty).unwrap();
        let repaired = serialized_inventory(&path, cap);
        assert_eq!(repaired.status, InventoryStatus::Complete);
        assert!(repaired.bindings.is_empty());
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
        assert_eq!(
            serialized_inventory(&path, 1).status,
            InventoryStatus::Unavailable(InventoryFailure::DecodedByteLimit)
        );
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
