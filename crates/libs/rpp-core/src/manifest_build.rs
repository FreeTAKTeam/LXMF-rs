//! Publisher side: chunk files and sign a manifest (RC1 steps 1–3).

use std::io::Read;

use sha2::{Digest as _, Sha256};

use crate::cbor::{self, Value};
use crate::error::{Result, RppError};
use crate::identity::ReceiveDestination;
use crate::identity::{sha256, Destination, Digest, SigningIdentity};
use crate::manifest::{
    chunk_value, file_value, ChunkEntry, FileEntry, OwnerBinding, VerifiedManifest,
    MAX_CHUNK_BYTES, VERSION,
};

/// One file to publish: its chunk descriptors and whole-file digest.
#[derive(Debug, Clone)]
pub struct FileDraft {
    pub path: String,
    pub destination: Destination,
    pub media_type: Option<String>,
    chunks: Vec<ChunkEntry>,
    digest: Digest,
    size: u64,
}

impl FileDraft {
    /// Hashes `reader` in `chunk_size` pieces; only one chunk is in memory.
    pub fn from_reader(
        path: impl Into<String>,
        destination: Destination,
        mut reader: impl Read,
        chunk_size: usize,
    ) -> Result<Self> {
        if chunk_size == 0 || chunk_size as u64 > MAX_CHUNK_BYTES {
            return Err(RppError::LimitExceeded);
        }
        let mut whole = Sha256::new();
        let mut chunks = Vec::new();
        let mut size = 0u64;
        let mut buffer = vec![0u8; chunk_size];
        loop {
            let mut filled = 0;
            while filled < chunk_size {
                let read = reader.read(&mut buffer[filled..])?;
                if read == 0 {
                    break;
                }
                filled += read;
            }
            if filled == 0 {
                break;
            }
            whole.update(&buffer[..filled]);
            chunks.push(ChunkEntry { len: filled as u64, digest: sha256(&buffer[..filled]) });
            size += filled as u64;
            if filled < chunk_size {
                break;
            }
        }
        Ok(Self {
            path: path.into(),
            destination,
            media_type: None,
            chunks,
            digest: whole.finalize().into(),
            size,
        })
    }

    /// Uses caller-chosen chunk boundaries (fixtures, application framing).
    pub fn from_chunks(
        path: impl Into<String>,
        destination: Destination,
        chunks: &[&[u8]],
    ) -> Self {
        let mut whole = Sha256::new();
        chunks.iter().for_each(|chunk| whole.update(chunk));
        Self {
            path: path.into(),
            destination,
            media_type: None,
            chunks: chunks
                .iter()
                .map(|chunk| ChunkEntry { len: chunk.len() as u64, digest: sha256(chunk) })
                .collect(),
            digest: whole.finalize().into(),
            size: chunks.iter().map(|chunk| chunk.len() as u64).sum(),
        }
    }
}

/// Everything a publisher chooses before signing. Destinations must be
/// allocated (RDO tokens persisted) before calling [`ManifestDraft::sign`].
#[derive(Debug, Clone, Default)]
pub struct ManifestDraft {
    pub package_destination: Destination,
    pub manifest_destination: Destination,
    pub files: Vec<FileDraft>,
    pub created: Option<u64>,
    pub expires: Option<u64>,
    pub predecessors: Vec<Digest>,
    pub sequence: Option<u64>,
    pub content_type: Option<String>,
    pub display_name: Option<String>,
    pub content_version: Option<String>,
    pub application_profile: Option<String>,
}

impl ManifestDraft {
    pub fn sign(mut self, owner: &SigningIdentity) -> Result<VerifiedManifest> {
        self.files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
        let mut file_values = Vec::with_capacity(self.files.len());
        let mut chunk_values = Vec::new();
        let mut total = 0u64;
        for file in &self.files {
            let first = chunk_values.len() as u32;
            for chunk in &file.chunks {
                chunk_values.push(chunk_value(chunk_values.len(), chunk));
            }
            total = total.checked_add(file.size).ok_or(RppError::LimitExceeded)?;
            file_values.push(file_value(&FileEntry {
                path: file.path.clone(),
                size: file.size,
                digest: file.digest,
                first_chunk: first,
                chunk_count: file.chunks.len() as u32,
                destination: file.destination,
                media_type: file.media_type.clone(),
            }));
        }
        let text = |value: Option<String>| value.map(Value::Text);
        let mut map = cbor::map([
            (0, Some(Value::Uint(0))),
            (1, Some(Value::Uint(VERSION))),
            (4, Some(Value::Bytes(owner.public().receive_destination().to_vec()))),
            (6, self.created.map(Value::Uint)),
            (8, Some(Value::Bytes(self.package_destination.to_vec()))),
            (9, Some(Value::Bytes(self.manifest_destination.to_vec()))),
            (12, self.expires.map(Value::Uint)),
            (
                14,
                (!self.predecessors.is_empty()).then(|| {
                    Value::Array(
                        self.predecessors.iter().map(|d| Value::Bytes(d.to_vec())).collect(),
                    )
                }),
            ),
            (15, self.sequence.map(Value::Uint)),
            (20, Some(Value::Uint(total))),
            (24, Some(Value::Uint(chunk_values.len() as u64))),
            (25, text(self.content_type)),
            (31, Some(Value::Array(file_values))),
            (32, Some(Value::Array(chunk_values))),
            (33, text(self.display_name)),
            (34, text(self.content_version)),
            (35, text(self.application_profile)),
        ])
        .into_map()?;
        let package_id = sha256(&cbor::encode(&Value::Map(map.clone())));
        map.insert(2, Value::Bytes(package_id.to_vec()));
        let signature = owner.sign(&cbor::encode(&Value::Map(map.clone())));
        map.insert(7, Value::Bytes(signature.to_vec()));
        let bytes = cbor::encode(&Value::Map(map));
        // One validation path for local and remote manifests.
        VerifiedManifest::verify(
            &bytes,
            &OwnerBinding {
                owner: owner.public(),
                package_destination: self.package_destination,
                manifest_destination: self.manifest_destination,
            },
        )
    }
}
