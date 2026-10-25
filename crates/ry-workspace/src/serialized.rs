//! Enumerate serialized R bindings without evaluating R code.

use super::file_stem_binding;
use std::cell::Cell;
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Inventory of a serialized R data file (`.rda`/`.rdata`). `bindings`
/// are the enumerated object names, or a conservative fallback when
/// enumeration fails; `failure` is set when they are not the full inventory.
#[derive(Clone)]
pub(super) struct SerializedInventory {
    pub(super) bindings: HashSet<String>,
    pub(super) failure: Option<InventoryFailure>,
}

/// Why a serialized inventory is unavailable. Only resource limits justify
/// the file-stem fallback binding.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum InventoryFailure {
    DecodedByteLimit,
    ParserResourceLimit,
    /// A valid serialization this reader does not handle (ASCII or native
    /// binary encodings, a non-pairlist object, or a stream the parser
    /// stops reading before its end).
    Unsupported,
    Unreadable,
}

impl InventoryFailure {
    pub fn description(self) -> &'static str {
        match self {
            Self::DecodedByteLimit => "decoded-byte limit exceeded",
            Self::ParserResourceLimit => "serialized parser resource limit exceeded",
            Self::Unsupported => "unsupported serialized input",
            Self::Unreadable => "serialized input could not be read",
        }
    }

    /// The user-facing notice for a degraded scope at `path`.
    pub fn notice(self, path: &Path) -> String {
        let hint = if self == Self::DecodedByteLimit {
            " Raise max-serialized-bytes in ry.toml to enumerate it."
        } else {
            ""
        };
        format!(
            "ry: {}: degraded scope ({}); serialized inventory unavailable.{hint}",
            path.display(),
            self.description()
        )
    }
}

/// Read only the top-level tags from an R serialization stream. `.rda`
/// workspaces are serialized pairlists whose tags are the binding names. The
/// parser's lazy mode skips vector payload allocation, and bzip2 streams are
/// decompressed in-process; no R runtime or project code is executed.
///
/// Resource limits use a single file-stem fallback ([`file_stem_binding`])
/// so unbound-variable analysis (RY010) stays live. Other failures keep the
/// empty-binding fallback (or, for a stream read only partly, the names read
/// so far) but are reported as unavailable rather than known complete.
/// `None` means the path does not exist.
pub(super) fn serialized_inventory(path: &Path, cap: u64) -> Option<SerializedInventory> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => return Some(unavailable(InventoryFailure::Unreadable)),
    };
    let mut inventory = cached_inventory(path, cap, file);
    if matches!(
        inventory.failure,
        Some(InventoryFailure::DecodedByteLimit | InventoryFailure::ParserResourceLimit)
    ) {
        inventory.bindings = file_stem_binding(path);
    }
    Some(inventory)
}

pub(super) fn unavailable(failure: InventoryFailure) -> SerializedInventory {
    SerializedInventory {
        bindings: HashSet::new(),
        failure: Some(failure),
    }
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
    let bytes = match read_serialized(file, cap) {
        Ok(bytes) => bytes,
        Err(failure) => return unavailable(failure),
    };
    let payload = bytes
        .strip_prefix(b"RDX2\n")
        .or_else(|| bytes.strip_prefix(b"RDX3\n"))
        .unwrap_or(&bytes);
    // The reader accepts XDR only; name R's other valid encodings instead of
    // letting the parser's generic error call them unreadable.
    if [b"RDA2\n".as_slice(), b"RDA3\n", b"RDB2\n", b"RDB3\n"]
        .iter()
        .any(|magic| bytes.starts_with(magic))
        || payload.starts_with(b"A\n")
        || payload.starts_with(b"B\n")
    {
        return unavailable(InventoryFailure::Unsupported);
    }
    let input = TrackedInput {
        bytes: payload,
        read_end: Cell::new(0),
    };
    let parsed = match rds2rust::read_rds_lazy_with_input(&input) {
        Ok(parsed) => Some(parsed),
        Err(error) if is_resource_limit(&error) => {
            // The tag walker skips values instead of materializing them,
            // so a stream beyond the parser's nesting limit can still
            // enumerate; only if it also fails is this a resource limit.
            return fallback_to_tag_walk(payload, InventoryFailure::ParserResourceLimit);
        }
        Err(_) => None,
    };
    let Some(parsed) = parsed else {
        // rds2rust 0.3 desynchronizes on an external pointer's payload and
        // can fail outright; the tag walker below reads the format the way
        // R writes it and recovers the full inventory (#714).
        return fallback_to_tag_walk(payload, InventoryFailure::Unreadable);
    };
    let bindings = match parsed.object.into_concrete() {
        rds2rust::RObject::Pairlist(elements) => elements
            .into_iter()
            .filter_map(|element| element.tag.map(|tag| tag.to_string()))
            .collect(),
        rds2rust::RObject::Null => HashSet::new(),
        _ => return unavailable(InventoryFailure::Unsupported),
    };
    // A workspace ends with its pairlist terminator. rds2rust 0.3 leaves an
    // external pointer's payload unread and stops at a premature terminator,
    // so a stream it did not read to the end is only a partial inventory.
    // Recover the missing names with the tag walker; if that also fails,
    // keep the names read so far but report the scope as degraded.
    if input.read_end.get() == payload.len() {
        return SerializedInventory {
            bindings,
            failure: None,
        };
    }
    match enumerate_workspace_tags(payload) {
        // The walker only succeeds on a byte-exact walk to the terminator,
        // so its tags are the complete set; names the desynchronized
        // parser read past an external pointer are not evidence and must
        // not survive into an inventory reported as complete.
        Some(full) => SerializedInventory {
            bindings: full,
            failure: None,
        },
        None => SerializedInventory {
            bindings,
            failure: Some(InventoryFailure::Unsupported),
        },
    }
}

/// Recover an inventory with the tag walker, or report `failure`.
fn fallback_to_tag_walk(payload: &[u8], failure: InventoryFailure) -> SerializedInventory {
    match enumerate_workspace_tags(payload) {
        Some(bindings) => SerializedInventory {
            bindings,
            failure: None,
        },
        None => unavailable(failure),
    }
}

/// Parser input over the decoded stream that records how far it was read.
struct TrackedInput<'a> {
    bytes: &'a [u8],
    read_end: Cell<usize>,
}

impl rds2rust::RdsInput for TrackedInput<'_> {
    fn read_at(&self, offset: u64, len: usize) -> rds2rust::Result<Vec<u8>> {
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        let end = start.saturating_add(len);
        let chunk = self
            .bytes
            .get(start..end)
            .ok_or(rds2rust::Error::UnexpectedEofDetail {
                position: start,
                needed: len,
                available: self.bytes.len().saturating_sub(start),
            })?;
        self.read_end.set(self.read_end.get().max(end));
        Ok(chunk.to_vec())
    }

    fn len(&self) -> Option<u64> {
        Some(self.bytes.len() as u64)
    }
}

/// rds2rust 0.3 reports its parser limits only as `InvalidFormat` text.
fn is_resource_limit(error: &rds2rust::Error) -> bool {
    match error {
        rds2rust::Error::MemoryBudgetExceeded { .. } => true,
        rds2rust::Error::InvalidFormat(message) => {
            message.starts_with("Parser nesting limit ")
                || message.starts_with("Materialized allocation of ")
                || message.contains(" bytes exceeds cap")
                || message.contains(" exceeds safe limit ")
                || message.contains(" overflow while parsing ")
        }
        _ => false,
    }
}

/// Enumerate a workspace's top-level pairlist tags by walking the XDR
/// stream the way R's serializer writes it, skipping every value. This is
/// the fallback for streams rds2rust 0.3 cannot finish: it returns NULL
/// for an external pointer without consuming the protected-value and tag
/// items R writes after it, which desynchronizes everything that follows
/// (#714). The result is trusted only when the walk ends exactly at the
/// pairlist terminator with no bytes left over; any surprise yields
/// `None` and the caller keeps its degraded outcome. Payload shapes mirror
/// `WriteItem`/`WriteBC` in R's serialize.c (format versions 2 and 3).
fn enumerate_workspace_tags(payload: &[u8]) -> Option<HashSet<String>> {
    let mut walker = TagWalker {
        bytes: payload,
        pos: 0,
        refs: Vec::new(),
    };
    walker.enumerate().ok()
}

/// Bounds-checked cursor over an XDR serialization stream. Only symbol
/// names are materialized (pairlist tags and the reference table); every
/// other payload is skipped by length. Nesting is depth-capped and
/// pairlist spines are walked iteratively, so hostile input can fail the
/// walk but cannot overflow the stack or loop without consuming bytes.
struct TagWalker<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// Reference table in encounter order (1-based on the wire). Only
    /// symbols keep their name, shared so a stream with millions of
    /// references to one long-named symbol does not copy it per use;
    /// other reference objects hold `None`.
    refs: Vec<Option<std::rc::Rc<str>>>,
}

const MAX_WALK_DEPTH: usize = 256;
const HAS_ATTR: u32 = 1 << 9;
const HAS_TAG: u32 = 1 << 10;

impl<'a> TagWalker<'a> {
    fn enumerate(&mut self) -> Result<HashSet<String>, ()> {
        if self.take(2)? != b"X\n" {
            return Err(());
        }
        let version = self.i32()?;
        self.i32()?;
        self.i32()?;
        if version == 3 {
            let len = usize::try_from(self.i32()?).map_err(|_| ())?;
            self.take(len)?;
        } else if version != 2 {
            return Err(());
        }
        let mut names = HashSet::new();
        loop {
            let word = self.u32()?;
            match word & 0xff {
                254 => break, // NILVALUE terminator
                2 => {
                    if word & HAS_ATTR != 0 {
                        self.skip_item(1)?;
                    }
                    if word & HAS_TAG != 0 {
                        // A tag that is not a symbol contributes no
                        // binding name, like the parser path's filter.
                        if let Some(tag) = self.skip_item(1)? {
                            names.insert(tag.to_string());
                        }
                    }
                    self.skip_item(1)?; // the bound value
                }
                _ => return Err(()),
            }
        }
        if self.pos != self.bytes.len() {
            return Err(());
        }
        Ok(names)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], ()> {
        let end = self.pos.checked_add(len).ok_or(())?;
        let chunk = self.bytes.get(self.pos..end).ok_or(())?;
        self.pos = end;
        Ok(chunk)
    }

    fn u32(&mut self) -> Result<u32, ()> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, ()> {
        Ok(self.u32()? as i32)
    }

    fn peek_u32(&self) -> Result<u32, ()> {
        let end = self.pos.checked_add(4).ok_or(())?;
        Ok(u32::from_be_bytes(
            self.bytes.get(self.pos..end).ok_or(())?.try_into().unwrap(),
        ))
    }

    /// R writes a vector length as one signed integer, or `-1` followed by
    /// a big-endian 64-bit count for long vectors.
    fn vec_len(&mut self) -> Result<usize, ()> {
        let first = self.i32()?;
        if first == -1 {
            let hi = u64::from(self.u32()?);
            let lo = u64::from(self.u32()?);
            usize::try_from((hi << 32) | lo).map_err(|_| ())
        } else {
            usize::try_from(first).map_err(|_| ())
        }
    }

    fn skip_bytes(&mut self, count: usize, width: usize) -> Result<(), ()> {
        let len = count.checked_mul(width).ok_or(())?;
        self.take(len).map(|_| ())
    }

    /// Each skipped element occupies at least one word, so a count beyond
    /// the remaining words cannot be real.
    fn check_count(&self, count: usize) -> Result<(), ()> {
        if count > (self.bytes.len() - self.pos) / 4 {
            return Err(());
        }
        Ok(())
    }

    /// Skip one serialized item. Returns the symbol name for SYMSXP items
    /// and references to earlier symbols, `None` for everything else.
    /// Pairlist-family CDRs (and a closure's body) are tail positions in
    /// R's writer, so they continue the loop instead of recursing; a
    /// tail item that is itself a symbol therefore propagates its name,
    /// which only tag positions may read.
    fn skip_item(&mut self, depth: usize) -> Result<Option<std::rc::Rc<str>>, ()> {
        if depth > MAX_WALK_DEPTH {
            return Err(());
        }
        loop {
            let word = self.u32()?;
            let typ = word & 0xff;
            let has_attr = word & HAS_ATTR != 0;
            let has_tag = word & HAS_TAG != 0;
            match typ {
                // Reference to an earlier reference object: the index is
                // packed above the type byte, or follows when it is zero.
                255 => {
                    let mut index = word >> 8;
                    if index == 0 {
                        index = self.u32()?;
                    }
                    let slot = usize::try_from(index)
                        .map_err(|_| ())?
                        .checked_sub(1)
                        .ok_or(())?;
                    return Ok(self.refs.get(slot).ok_or(())?.clone());
                }
                // Bare singletons: base/empty/global environments, base
                // namespace, missing argument, unbound value, NILVALUE.
                241 | 242 | 250 | 251 | 252 | 253 | 254 => return Ok(None),
                // SYMSXP: a bare type word and the print-name CHARSXP.
                1 => {
                    let slot = self.refs.len();
                    self.refs.push(None);
                    let name = self.charsxp_bytes()?.map(|bytes| -> std::rc::Rc<str> {
                        match String::from_utf8_lossy(bytes) {
                            std::borrow::Cow::Borrowed(text) => text.into(),
                            std::borrow::Cow::Owned(text) => text.into_boxed_str().into(),
                        }
                    });
                    self.refs[slot] = name.clone();
                    return Ok(name);
                }
                // ENVSXP: bare type word, locked flag, enclosure, frame,
                // hash table, and attributes (written unconditionally).
                4 => {
                    self.refs.push(None);
                    self.u32()?;
                    for _ in 0..4 {
                        self.skip_item(depth + 1)?;
                    }
                    return Ok(None);
                }
                // PERSISTSXP / PACKAGESXP / NAMESPACESXP: bare type word
                // and a string vector with a placeholder integer first.
                247..=249 => {
                    self.refs.push(None);
                    self.u32()?;
                    let count = self.vec_len()?;
                    self.check_count(count)?;
                    for _ in 0..count {
                        self.charsxp_bytes()?;
                    }
                    return Ok(None);
                }
                // LISTSXP / LANGSXP / PROMSXP / DOTSXP: attributes and tag
                // first, then CAR, then CDR as the tail.
                2 | 6 | 5 | 17 => {
                    if has_attr {
                        self.skip_item(depth + 1)?;
                    }
                    if has_tag {
                        self.skip_item(depth + 1)?;
                    }
                    self.skip_item(depth + 1)?;
                }
                // CLOSXP: attributes, environment, formals, body as tail.
                3 => {
                    if has_attr {
                        self.skip_item(depth + 1)?;
                    }
                    self.skip_item(depth + 1)?;
                    self.skip_item(depth + 1)?;
                }
                _ => {
                    self.skip_payload(typ, has_attr, depth)?;
                    return Ok(None);
                }
            }
        }
    }

    /// Skip the payload of a non-pairlist item (header word consumed),
    /// then its attributes unless the type writes them itself.
    fn skip_payload(&mut self, typ: u32, has_attr: bool, depth: usize) -> Result<(), ()> {
        match typ {
            0 | 25 => {} // NILSXP, S4: nothing but attributes
            // SPECIALSXP / BUILTINSXP: a length-prefixed primitive name.
            7 | 8 => {
                let len = usize::try_from(self.i32()?).map_err(|_| ())?;
                self.take(len)?;
            }
            9 => {
                // CHARSXP: only -1 (NA_STRING) may be negative.
                let len = self.i32()?;
                if len != -1 {
                    self.take(usize::try_from(len).map_err(|_| ())?)?;
                }
            }
            10 | 13 => {
                let count = self.vec_len()?;
                self.skip_bytes(count, 4)?;
            }
            14 => {
                let count = self.vec_len()?;
                self.skip_bytes(count, 8)?;
            }
            15 => {
                let count = self.vec_len()?;
                self.skip_bytes(count, 16)?;
            }
            16 => {
                let count = self.vec_len()?;
                self.check_count(count)?;
                for _ in 0..count {
                    self.charsxp_bytes()?;
                }
            }
            19 | 20 => {
                let count = self.vec_len()?;
                self.check_count(count)?;
                for _ in 0..count {
                    self.skip_item(depth + 1)?;
                }
            }
            21 => self.skip_bytecode(depth)?,
            // EXTPTRSXP: protected value and tag items follow.
            22 => {
                self.refs.push(None);
                self.skip_item(depth + 1)?;
                self.skip_item(depth + 1)?;
            }
            // WEAKREFSXP: registered, but R writes no payload for it.
            23 => self.refs.push(None),
            24 => {
                let count = self.vec_len()?;
                self.skip_bytes(count, 1)?;
            }
            // ALTREP: info, state, and attributes, all unconditional.
            238 => {
                for _ in 0..3 {
                    self.skip_item(depth + 1)?;
                }
                return Ok(());
            }
            _ => return Err(()),
        }
        if has_attr {
            self.skip_item(depth + 1)?;
        }
        Ok(())
    }

    /// Skip a CHARSXP item, returning its borrowed bytes (`None` for
    /// NA_STRING). Callers that only skip never materialize the text.
    fn charsxp_bytes(&mut self) -> Result<Option<&'a [u8]>, ()> {
        let word = self.u32()?;
        if word & 0xff != 9 {
            return Err(());
        }
        let len = self.i32()?;
        if len == -1 {
            return Ok(None);
        }
        Ok(Some(self.take(usize::try_from(len).map_err(|_| ())?)?))
    }

    /// BCODESXP: a rep count, then code and constants in the bytecode
    /// writer's own grammar (bare type words, rep markers, 0-pad leaves).
    fn skip_bytecode(&mut self, depth: usize) -> Result<(), ()> {
        self.u32()?;
        self.skip_bc_body(depth + 1)
    }

    fn skip_bc_body(&mut self, depth: usize) -> Result<(), ()> {
        if depth > MAX_WALK_DEPTH {
            return Err(());
        }
        self.skip_item(depth + 1)?; // instruction stream (INTSXP item)
        let count = usize::try_from(self.i32()?).map_err(|_| ())?;
        self.check_count(count)?;
        for _ in 0..count {
            match self.peek_u32()? {
                21 => {
                    self.u32()?;
                    self.skip_bc_body(depth + 1)?;
                }
                2 | 6 | 239 | 240 | 243 | 244 => self.skip_bc_lang(depth + 1)?,
                _ => {
                    self.u32()?;
                    self.skip_item(depth + 1)?;
                }
            }
        }
        Ok(())
    }

    /// One language/pairlist node inside bytecode. Shared nodes are
    /// wrapped in BCREPDEF/BCREPREF markers; non-node leaves are a zero
    /// pad word followed by a full item. CDR is the tail.
    fn skip_bc_lang(&mut self, depth: usize) -> Result<(), ()> {
        if depth > MAX_WALK_DEPTH {
            return Err(());
        }
        loop {
            let mut word = self.u32()?;
            if word == 243 {
                self.u32()?;
                return Ok(());
            }
            if word == 244 {
                self.u32()?;
                word = self.u32()?;
            }
            match word {
                0 => {
                    self.skip_item(depth + 1)?;
                    return Ok(());
                }
                239 | 240 => {
                    self.skip_item(depth + 1)?; // attributes
                }
                2 | 6 => {}
                _ => return Err(()),
            }
            self.skip_item(depth + 1)?; // tag, always written here
            self.skip_bc_lang(depth + 1)?; // CAR
        }
    }
}

fn read_serialized(reader: impl std::io::Read, cap: u64) -> Result<Vec<u8>, InventoryFailure> {
    let mut reader = reader;
    let mut prefix = Vec::with_capacity(6);
    if reader.by_ref().take(6).read_to_end(&mut prefix).is_err() {
        return Err(InventoryFailure::Unreadable);
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

/// Decode `decoder` fully, stopping once the payload provably exceeds
/// `cap`. Reading at most `cap + 1` bytes distinguishes an
/// exact-cap payload from a larger one without decoding all of it.
fn decode_capped(decoder: impl std::io::Read, cap: u64) -> Result<Vec<u8>, InventoryFailure> {
    let mut decoded = Vec::new();
    if decoder
        .take(cap.saturating_add(1))
        .read_to_end(&mut decoded)
        .is_err()
    {
        return Err(InventoryFailure::Unreadable);
    }
    if decoded.len() as u64 > cap {
        return Err(InventoryFailure::DecodedByteLimit);
    }
    Ok(decoded)
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
            assert!(matches!(read_serialized(&mut reader, 256), Ok(result) if result == payload));
            assert!(matches!(
                read_serialized(bytes.as_slice(), 255),
                Err(InventoryFailure::DecodedByteLimit)
            ));
        }
        let bytes = vec![b'x'; 65536];
        let mut reader = TinyReads {
            bytes: &bytes,
            consumed: 0,
        };
        assert!(matches!(
            read_serialized(&mut reader, 16),
            Err(InventoryFailure::DecodedByteLimit)
        ));
        assert_eq!(reader.consumed, 17);
        for corrupt in [
            b"BZh".as_slice(),
            &[0x1f, 0x8b],
            &[0xfd, b'7', b'z', b'X', b'Z', 0],
        ] {
            assert!(matches!(
                read_serialized(corrupt, 256),
                Err(InventoryFailure::Unreadable)
            ));
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
        // and re-flags all internal lookup tables as unbound.
        let cap = ry_config::Config::default().max_serialized_bytes;
        let (_guard, path) = gzipped_sysdata(&[
            ("locales", ProbeValue::Doubles(200_000)),
            ("currencies", ProbeValue::Doubles(200_000)),
        ]);
        let inventory = serialized_inventory(&path, cap).unwrap();
        assert_eq!(inventory.failure, None, "3.2 MB workspace must enumerate");
        assert!(inventory.bindings.contains("locales"));
        assert!(inventory.bindings.contains("currencies"));
    }

    #[test]
    fn explicit_small_cap_still_degrades_to_the_file_stem() {
        // An explicit cap always wins over the raised default: users keep a
        // hard bound for adversarial or huge files.
        let (_guard, path) = gzipped_sysdata(&[("locales", ProbeValue::Doubles(4))]);
        let inventory = serialized_inventory(&path, 64).unwrap();
        assert_eq!(inventory.failure, Some(InventoryFailure::DecodedByteLimit));
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
        let inventory = serialized_inventory(&path, cap).unwrap();
        assert_eq!(inventory.failure, Some(InventoryFailure::DecodedByteLimit));
        assert_eq!(inventory.bindings, HashSet::from(["sysdata".to_string()]));
    }

    #[test]
    fn malformed_stream_is_reported_without_a_stem_fallback() {
        // Invalid bytes do not justify inventing the file-stem binding, but
        // must not be presented as a successfully inventoried empty file.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sysdata.rda");
        std::fs::write(&path, b"not a serialization stream").expect("write garbage");
        let inventory = serialized_inventory(&path, 1 << 20).unwrap();
        assert_eq!(inventory.failure, Some(InventoryFailure::Unreadable));
        assert!(inventory.bindings.is_empty());
    }

    #[test]
    fn parser_limit_messages_are_limits_and_truncation_is_not() {
        // The InvalidFormat message families of rds2rust 0.3; a reword
        // upstream would surface as a lost file-stem fallback.
        for (message, limit) in [
            ("Parser nesting limit 64 exceeded", true),
            (
                "Materialized allocation of 1 bytes exceeds limits while parsing x",
                true,
            ),
            ("Allocation of 1 bytes exceeds cap while parsing x", true),
            ("Length 1 exceeds safe limit 1 while parsing x", true),
            ("Allocation size overflow while parsing x", true),
            ("Length overflow while parsing x", true),
            (
                "Length 1 (1 bytes) exceeds remaining 1 bytes while parsing x",
                false,
            ),
            ("invalid R serialization header", false),
        ] {
            let error = rds2rust::Error::InvalidFormat(message.into());
            assert_eq!(is_resource_limit(&error), limit, "{message}");
        }
        assert!(is_resource_limit(&rds2rust::Error::MemoryBudgetExceeded {
            needed: 2,
            available: 1,
        }));
        assert!(!is_resource_limit(&rds2rust::Error::ParseError(
            "opaque".into()
        )));
    }

    #[test]
    fn empty_workspace_and_parser_limit_have_distinct_inventory_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sysdata.rda");
        let cap = 4096;
        assert!(serialized_inventory(&path, cap).is_none());
        let empty = include_bytes!("../../../testdata/serialized/empty.rda");
        std::fs::write(&path, empty).unwrap();
        let inventory = serialized_inventory(&path, cap).unwrap();
        assert_eq!(inventory.failure, None);
        assert!(inventory.bindings.is_empty());

        // Seventy nested lists exceed the full parser's nesting limit of
        // 64, but the tag walker only skips payloads, so it still
        // enumerates the workspace the parser had to give up on.
        let nested = include_bytes!("../../../testdata/serialized/nested-limit.rda");
        assert!(nested.len() < cap as usize);
        assert!(matches!(
            read_serialized(nested.as_slice(), cap),
            Ok(bytes) if bytes.len() < cap as usize
        ));
        std::fs::write(&path, nested).unwrap();
        let inventory = serialized_inventory(&path, cap).unwrap();
        assert_eq!(inventory.failure, None);
        assert_eq!(inventory.bindings, HashSet::from(["x".to_string()]));

        // Three hundred levels also exceed the walker's depth cap, which
        // is a genuine resource limit: degraded, with the file stem.
        let deep = include_bytes!("../../../testdata/serialized/nested-deep.rda");
        std::fs::write(&path, deep).unwrap();
        let inventory = serialized_inventory(&path, cap).unwrap();
        assert_eq!(
            inventory.failure,
            Some(InventoryFailure::ParserResourceLimit)
        );
        assert_eq!(inventory.bindings, HashSet::from(["sysdata".to_string()]));

        std::fs::write(&path, empty).unwrap();
        let repaired = serialized_inventory(&path, cap).unwrap();
        assert_eq!(repaired.failure, None);
        assert!(repaired.bindings.is_empty());
    }

    #[test]
    fn external_pointer_workspaces_enumerate_completely() {
        // rds2rust 0.3 leaves an external pointer's payload unread and ends
        // the workspace early (#714); the tag walker recovers every name,
        // including the ones saved after the pointer.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sysdata.rda");
        let extptr = include_bytes!("../../../testdata/serialized/extptr.rda");
        std::fs::write(&path, extptr).unwrap();
        let inventory = serialized_inventory(&path, 4096).unwrap();
        assert_eq!(inventory.failure, None);
        assert_eq!(
            inventory.bindings,
            HashSet::from(["a_first".into(), "ptr_value".into(), "z_last".into()])
        );

        // The gtsummary shape: an external pointer nested in the first
        // object's attributes, plus ALTREP, bytecode, and an environment,
        // must not hide the bindings saved after them either.
        let nested = include_bytes!("../../../testdata/serialized/extptr-nested.rda");
        std::fs::write(&path, nested).unwrap();
        let inventory = serialized_inventory(&path, 4096).unwrap();
        assert_eq!(inventory.failure, None);
        assert_eq!(
            inventory.bindings,
            HashSet::from([
                "df_like".into(),
                "seq_like".into(),
                "fn_like".into(),
                "env_like".into(),
                "z_last".into(),
            ])
        );
    }

    #[test]
    fn tag_walker_rejects_truncated_and_overlong_streams() {
        // The walker's result is trusted only at exact end of stream: a
        // truncated workspace and one with trailing garbage both stay
        // degraded instead of passing for complete.
        let full = rda_v2_workspace(&[("only", ProbeValue::Doubles(1))]);
        let payload = full.strip_prefix(b"RDX2\n").unwrap();
        assert_eq!(
            enumerate_workspace_tags(payload),
            Some(HashSet::from(["only".into()]))
        );
        assert_eq!(
            enumerate_workspace_tags(&payload[..payload.len() - 1]),
            None
        );
        let mut overlong = payload.to_vec();
        overlong.push(0);
        assert_eq!(enumerate_workspace_tags(&overlong), None);
        assert_eq!(enumerate_workspace_tags(b""), None);
        assert_eq!(enumerate_workspace_tags(b"X\n"), None);
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
            serialized_inventory(&path, 1).unwrap().failure,
            Some(InventoryFailure::DecodedByteLimit)
        );
        assert!(
            serialized_inventory(&path, 4096)
                .unwrap()
                .bindings
                .contains("first")
        );
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
            serialized_inventory(&path, 4096).unwrap().bindings,
            HashSet::from(["other".into()])
        );
        #[cfg(unix)]
        {
            let alias = dir.path().join("alias.rda");
            std::os::unix::fs::symlink(&path, &alias).unwrap();
            assert!(
                serialized_inventory(&alias, 4096)
                    .unwrap()
                    .bindings
                    .contains("other")
            );
            assert_eq!(
                serialized_inventory(&alias, 1).unwrap().bindings,
                HashSet::from(["alias".into()])
            );
            assert_eq!(
                serialized_inventory(&path, 1).unwrap().bindings,
                HashSet::from(["data".into()])
            );
        }
    }

    #[test]
    fn only_the_byte_limit_notice_suggests_raising_the_cap() {
        let path = Path::new("/project/R/sysdata.rda");
        let byte_limit = InventoryFailure::DecodedByteLimit.notice(path);
        assert!(byte_limit.contains("R/sysdata.rda"));
        assert!(byte_limit.contains("max-serialized-bytes in ry.toml"));
        let parser_limit = InventoryFailure::ParserResourceLimit.notice(path);
        assert!(parser_limit.contains("serialized parser resource limit exceeded"));
        assert!(!parser_limit.contains("Raise max-serialized-bytes"));
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
        let inventory = serialized_inventory(&path, 1 << 20).unwrap();
        assert_eq!(
            inventory.bindings,
            HashSet::from(["data_like".to_string(), "flag_like".to_string()])
        );
    }
}
