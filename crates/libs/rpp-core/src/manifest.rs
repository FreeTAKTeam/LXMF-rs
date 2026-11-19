//! Signed manifest decoding and verification (RC1 profile, "Manifest and
//! publication").

use std::collections::HashSet;

use crate::cbor::{self, Fields, Value};
use crate::error::{Result, RppError};
use crate::identity::{sha256, Destination, Digest, PublicIdentity, Signature};

pub const VERSION: u64 = 3;
pub const MAX_SIGNED_MANIFEST: usize = 4 * 1024 * 1024;
pub const MAX_FILES: usize = 4096;
pub const MAX_CHUNKS: usize = 65_536;
pub const MAX_PACKAGE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_CHUNK_BYTES: u64 = 1024 * 1024;
pub const MAX_EXTENSIONS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub digest: Digest,
    pub first_chunk: u32,
    pub chunk_count: u32,
    pub destination: Destination,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkEntry {
    pub len: u64,
    pub digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub package_id: Digest,
    pub receive_destination: Destination,
    pub signature: Signature,
    pub package_destination: Destination,
    pub manifest_destination: Destination,
    pub total_size: u64,
    pub files: Vec<FileEntry>,
    pub chunks: Vec<ChunkEntry>,
    /// Derived logical offset of each chunk in the package stream.
    pub chunk_offsets: Vec<u64>,
    pub created: Option<u64>,
    pub expires: Option<u64>,
    pub predecessors: Vec<Digest>,
    pub sequence: Option<u64>,
    pub content_type: Option<String>,
    pub display_name: Option<String>,
    pub content_version: Option<String>,
    pub application_profile: Option<String>,
}

/// The owner and object Destinations already resolved through the RDO
/// shared-Identity owner binding v1. The caller vouches for this binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerBinding {
    pub owner: PublicIdentity,
    pub package_destination: Destination,
    pub manifest_destination: Destination,
}

/// A manifest whose signature, package ID and inventory were verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedManifest {
    pub manifest: Manifest,
    pub bytes: Vec<u8>,
    /// External manifest hash H, SHA-256 of the exact signed bytes.
    pub hash: Digest,
}

impl VerifiedManifest {
    pub fn verify(bytes: &[u8], binding: &OwnerBinding) -> Result<Self> {
        let value = cbor::decode(bytes, MAX_SIGNED_MANIFEST, false)?;
        let mut map = value.into_map()?;
        let manifest = parse_manifest(Fields::new(map.clone()))?;

        map.remove(&7);
        binding.owner.verify(&cbor::encode(&Value::Map(map.clone())), &manifest.signature)?;
        map.remove(&2);
        if sha256(&cbor::encode(&Value::Map(map))) != manifest.package_id {
            return Err(RppError::InvalidManifest("package ID does not match content"));
        }
        if manifest.package_destination != binding.package_destination
            || manifest.manifest_destination != binding.manifest_destination
        {
            return Err(RppError::InvalidManifest("object Destinations differ from owner binding"));
        }
        if manifest.receive_destination != binding.owner.receive_destination() {
            return Err(RppError::InvalidManifest("publisher inbox not derived from owner"));
        }
        Ok(Self { manifest, bytes: bytes.to_vec(), hash: sha256(bytes) })
    }

    pub fn chunk_count(&self) -> u32 {
        self.manifest.chunks.len() as u32
    }
}

/// Validates keys 10/11. RC1 assigns no extension IDs, so any critical ID is
/// unknown and refused; noncritical entries are tolerated.
pub(crate) fn check_extensions(fields: &mut Fields) -> Result<()> {
    let extensions = fields.take(10).map(Value::into_map).transpose()?;
    if extensions.as_ref().is_some_and(|map| map.len() > MAX_EXTENSIONS) {
        return Err(RppError::LimitExceeded);
    }
    let Some(critical) = fields.take(11) else { return Ok(()) };
    let critical = critical.into_array()?;
    let mut previous = None;
    for id in &critical {
        let id = id.as_uint()?;
        if previous.is_some_and(|previous| id <= previous)
            || !extensions.as_ref().is_some_and(|map| map.contains_key(&id))
        {
            return Err(RppError::Malformed("critical IDs must be increasing and present"));
        }
        previous = Some(id);
    }
    match critical.first() {
        Some(id) => Err(RppError::UnsupportedExtension(id.as_uint()?)),
        None => Err(RppError::Malformed("empty critical ID array")),
    }
}

fn parse_manifest(mut fields: Fields) -> Result<Manifest> {
    if fields.uint(0)? != 0 {
        return Err(RppError::Malformed("not a manifest"));
    }
    let version = fields.uint(1)?;
    if version != VERSION {
        return Err(RppError::UnsupportedVersion(version));
    }
    check_extensions(&mut fields)?;
    let package_id = fields.fixed(2)?;
    let receive_destination = fields.fixed(4)?;
    let signature = fields.fixed(7)?;
    let package_destination = fields.fixed(8)?;
    let manifest_destination = fields.fixed(9)?;
    let total_size = fields.uint(20)?;
    let chunk_count = fields.uint(24)?;
    let created = fields.opt_uint(6)?;
    let expires = fields.opt_uint(12)?;
    if let (Some(created), Some(expires)) = (created, expires) {
        if expires <= created {
            return Err(RppError::InvalidManifest("expiry must follow creation"));
        }
    }
    let predecessors = match fields.take(14) {
        None => Vec::new(),
        Some(value) => {
            let list = value
                .into_array()?
                .into_iter()
                .map(Value::into_fixed::<32>)
                .collect::<Result<Vec<_>>>()?;
            let distinct: HashSet<_> = list.iter().collect();
            if list.is_empty() || distinct.len() != list.len() {
                return Err(RppError::InvalidManifest(
                    "predecessors must be nonempty and distinct",
                ));
            }
            list
        }
    };
    let sequence = fields.opt_uint(15)?;
    let content_type = fields.opt_text(25)?;
    let display_name = fields.opt_text(33)?;
    let content_version = fields.opt_text(34)?;
    let application_profile = fields.opt_text(35)?;
    let raw_files = fields.array(31)?;
    let raw_chunks = fields.array(32)?;
    fields.finish()?;

    if raw_files.is_empty() || raw_files.len() > MAX_FILES || raw_chunks.len() > MAX_CHUNKS {
        return Err(RppError::InvalidManifest("file or chunk count out of bounds"));
    }
    if chunk_count != raw_chunks.len() as u64 {
        return Err(RppError::InvalidManifest("chunk count mismatch"));
    }
    let (chunks, chunk_offsets) = parse_chunks(raw_chunks)?;
    let sum =
        chunk_offsets.last().zip(chunks.last()).map_or(0, |(offset, chunk)| offset + chunk.len);
    if sum != total_size || total_size > MAX_PACKAGE_BYTES {
        return Err(RppError::InvalidManifest("total size mismatch"));
    }
    let files = parse_files(raw_files, &chunks, &chunk_offsets)?;
    Ok(Manifest {
        package_id,
        receive_destination,
        signature,
        package_destination,
        manifest_destination,
        total_size,
        files,
        chunks,
        chunk_offsets,
        created,
        expires,
        predecessors,
        sequence,
        content_type,
        display_name,
        content_version,
        application_profile,
    })
}

fn parse_chunks(raw: Vec<Value>) -> Result<(Vec<ChunkEntry>, Vec<u64>)> {
    let mut chunks = Vec::with_capacity(raw.len());
    let mut offsets = Vec::with_capacity(raw.len());
    let mut offset = 0u64;
    for (position, value) in raw.into_iter().enumerate() {
        let mut fields = Fields::new(value.into_map()?);
        let index = fields.uint(23)?;
        let len = fields.uint(20)?;
        let digest: Digest = fields.fixed(21)?;
        let echoed_digest = fields.opt_fixed::<32>(22)?;
        let echoed_offset = fields.opt_uint(44)?;
        let echoed_len = fields.opt_uint(45)?;
        fields.finish()?;
        if index != position as u64
            || len == 0
            || len > MAX_CHUNK_BYTES
            || echoed_digest.is_some_and(|echo| echo != digest)
            || echoed_offset.is_some_and(|echo| echo != offset)
            || echoed_len.is_some_and(|echo| echo != len)
        {
            return Err(RppError::InvalidManifest("invalid chunk descriptor"));
        }
        chunks.push(ChunkEntry { len, digest });
        offsets.push(offset);
        offset = offset.checked_add(len).ok_or(RppError::LimitExceeded)?;
    }
    Ok((chunks, offsets))
}

fn parse_files(raw: Vec<Value>, chunks: &[ChunkEntry], offsets: &[u64]) -> Result<Vec<FileEntry>> {
    let mut files: Vec<FileEntry> = Vec::with_capacity(raw.len());
    let mut destinations = HashSet::new();
    let mut directories = HashSet::new();
    let mut next_chunk = 0u64;
    for value in raw {
        let mut fields = Fields::new(value.into_map()?);
        let size = fields.uint(20)?;
        let path = fields.required(40)?.into_text()?;
        let digest: Digest = fields.fixed(41)?;
        let range = fields.array(43)?;
        let destination: Destination = fields.fixed(46)?;
        let media_type = fields.opt_text(25)?;
        let interval = fields.take(42).map(Value::into_array).transpose()?;
        fields.finish()?;

        let [first, count] = uint_pair(range)?;
        let end = first.checked_add(count).ok_or(RppError::LimitExceeded)?;
        if first != next_chunk || end > chunks.len() as u64 {
            return Err(RppError::InvalidManifest("files do not partition the chunk array"));
        }
        next_chunk = end;
        let (first_usize, end_usize) = (first as usize, end as usize);
        let file_size: u64 = chunks[first_usize..end_usize].iter().map(|chunk| chunk.len).sum();
        if file_size != size || (count == 0 && digest != sha256(b"")) {
            return Err(RppError::InvalidManifest("file size or empty-file digest mismatch"));
        }
        if let Some(interval) = interval {
            let start = offsets.get(first_usize).copied().unwrap_or_else(|| {
                offsets.last().zip(chunks.last()).map_or(0, |(offset, chunk)| offset + chunk.len)
            });
            if uint_pair(interval)? != [start, size] {
                return Err(RppError::InvalidManifest("file interval mismatch"));
            }
        }
        validate_path(&path)?;
        if files.last().is_some_and(|previous| previous.path.as_bytes() >= path.as_bytes())
            || !destinations.insert(destination)
        {
            return Err(RppError::InvalidManifest("files must be sorted with unique paths"));
        }
        for (slash, _) in path.match_indices('/') {
            directories.insert(path[..slash].to_owned());
        }
        files.push(FileEntry {
            path,
            size,
            digest,
            first_chunk: first as u32,
            chunk_count: count as u32,
            destination,
            media_type,
        });
    }
    if next_chunk != chunks.len() as u64 {
        return Err(RppError::InvalidManifest("chunks not covered by files"));
    }
    if files.iter().any(|file| directories.contains(&file.path)) {
        return Err(RppError::InvalidManifest("file path collides with a directory"));
    }
    Ok(files)
}

fn uint_pair(values: Vec<Value>) -> Result<[u64; 2]> {
    match values.as_slice() {
        [a, b] => Ok([a.as_uint()?, b.as_uint()?]),
        _ => Err(RppError::Malformed("expected a two-element array")),
    }
}

/// Portable extraction rule: slash-separated, no empty/dot/dot-dot
/// components, backslash, colon, NUL or ASCII control characters.
pub fn validate_path(path: &str) -> Result<()> {
    let bad_char = |c: char| c == '\\' || c == ':' || c.is_ascii_control();
    if path.is_empty()
        || path.chars().any(bad_char)
        || path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(RppError::InvalidManifest("unsafe file path"));
    }
    Ok(())
}

pub(crate) fn file_value(file: &FileEntry) -> Value {
    cbor::map([
        (20, Some(Value::Uint(file.size))),
        (25, file.media_type.clone().map(Value::Text)),
        (40, Some(Value::Text(file.path.clone()))),
        (41, Some(Value::Bytes(file.digest.to_vec()))),
        (43, Some(cbor::uint_array([u64::from(file.first_chunk), u64::from(file.chunk_count)]))),
        (46, Some(Value::Bytes(file.destination.to_vec()))),
    ])
}

pub(crate) fn chunk_value(index: usize, chunk: &ChunkEntry) -> Value {
    cbor::map([
        (20, Some(Value::Uint(chunk.len))),
        (21, Some(Value::Bytes(chunk.digest.to_vec()))),
        (23, Some(Value::Uint(index as u64))),
    ])
}
