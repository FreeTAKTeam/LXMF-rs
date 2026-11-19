//! Storage adapters: verified-manifest cache, private chunk staging and
//! atomic promotion (receiver), plus chunk sources (publisher).

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rand_core::{OsRng, RngCore};
use sha2::{Digest as _, Sha256};

use crate::error::{Result, RppError};
use crate::identity::{sha256, Digest};
use crate::manifest::VerifiedManifest;

/// Receiver persistence. Implement this for platform storage (Android, RCH).
pub trait ChunkStore {
    fn load_manifest(&self, hash: &Digest) -> Result<Option<Vec<u8>>>;
    fn save_manifest(&mut self, manifest: &VerifiedManifest) -> Result<()>;
    fn discard_manifest(&mut self, hash: &Digest) -> Result<()>;
    /// Re-verifies staged chunks against the manifest; invalid ones are dropped.
    fn verified_chunks(&mut self, manifest: &VerifiedManifest) -> Result<Vec<bool>>;
    /// Stores one chunk the caller already verified against the manifest.
    fn write_chunk(&mut self, manifest: &VerifiedManifest, index: u32, bytes: &[u8]) -> Result<()>;
    /// Checks every file digest and atomically publishes the package.
    fn promote(&mut self, manifest: &VerifiedManifest) -> Result<()>;
    fn is_promoted(&self, hash: &Digest) -> bool;
}

/// Publisher-side chunk reader.
pub trait PackageSource {
    fn read_chunk(&self, manifest: &VerifiedManifest, index: u32) -> Result<Vec<u8>>;
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Filesystem store: `manifests/<H>`, `staging/<H>/<index>`, `packages/<H>/…`.
pub struct FsStore {
    root: PathBuf,
}

impl FsStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        for dir in ["manifests", "staging", "packages", "tmp"] {
            fs::create_dir_all(root.join(dir))?;
        }
        Ok(Self { root })
    }

    pub fn package_dir(&self, hash: &Digest) -> PathBuf {
        self.root.join("packages").join(hex(hash))
    }

    fn staging(&self, hash: &Digest) -> PathBuf {
        self.root.join("staging").join(hex(hash))
    }

    fn atomic_write(&self, target: &Path, bytes: &[u8]) -> Result<()> {
        let tmp = self.root.join("tmp").join(format!("{:016x}", OsRng.next_u64()));
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, target)?;
        Ok(())
    }
}

impl ChunkStore for FsStore {
    fn load_manifest(&self, hash: &Digest) -> Result<Option<Vec<u8>>> {
        match fs::read(self.root.join("manifests").join(hex(hash))) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn save_manifest(&mut self, manifest: &VerifiedManifest) -> Result<()> {
        self.atomic_write(&self.root.join("manifests").join(hex(&manifest.hash)), &manifest.bytes)
    }

    fn discard_manifest(&mut self, hash: &Digest) -> Result<()> {
        match fs::remove_file(self.root.join("manifests").join(hex(hash))) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }

    fn verified_chunks(&mut self, manifest: &VerifiedManifest) -> Result<Vec<bool>> {
        let staging = self.staging(&manifest.hash);
        let mut present = vec![false; manifest.manifest.chunks.len()];
        for (index, chunk) in manifest.manifest.chunks.iter().enumerate() {
            let path = staging.join(index.to_string());
            match fs::read(&path) {
                Ok(bytes) if bytes.len() as u64 == chunk.len && sha256(&bytes) == chunk.digest => {
                    present[index] = true
                }
                Ok(_) => {
                    log::warn!(
                        "rpp: dropping corrupt staged chunk {index} of {}",
                        hex(&manifest.hash)
                    );
                    fs::remove_file(&path)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(present)
    }

    fn write_chunk(&mut self, manifest: &VerifiedManifest, index: u32, bytes: &[u8]) -> Result<()> {
        let staging = self.staging(&manifest.hash);
        fs::create_dir_all(&staging)?;
        self.atomic_write(&staging.join(index.to_string()), bytes)
    }

    fn promote(&mut self, manifest: &VerifiedManifest) -> Result<()> {
        let target = self.package_dir(&manifest.hash);
        if target.exists() {
            return Ok(());
        }
        let tmp = self.root.join("tmp").join(format!("promote-{:016x}", OsRng.next_u64()));
        let result = assemble(&self.staging(&manifest.hash), &tmp, manifest)
            .and_then(|()| fs::rename(&tmp, &target).map_err(RppError::from));
        if result.is_err() {
            let _ = fs::remove_dir_all(&tmp);
            return result;
        }
        fs::remove_dir_all(self.staging(&manifest.hash))?;
        Ok(())
    }

    fn is_promoted(&self, hash: &Digest) -> bool {
        self.package_dir(hash).is_dir()
    }
}

fn assemble(staging: &Path, out: &Path, verified: &VerifiedManifest) -> Result<()> {
    fs::create_dir_all(out)?;
    for file in &verified.manifest.files {
        // Paths were validated as portable relative paths; parents are fresh.
        let path = out.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        let mut digest = Sha256::new();
        for index in file.first_chunk..file.first_chunk + file.chunk_count {
            let bytes = fs::read(staging.join(index.to_string()))?;
            digest.update(&bytes);
            output.write_all(&bytes)?;
        }
        output.sync_all()?;
        if <[u8; 32]>::from(digest.finalize()) != file.digest {
            return Err(RppError::ChunkMismatch);
        }
    }
    Ok(())
}

/// Serves chunks straight from the published files under `root`.
pub struct DirSource {
    pub root: PathBuf,
}

impl PackageSource for DirSource {
    fn read_chunk(&self, verified: &VerifiedManifest, index: u32) -> Result<Vec<u8>> {
        let manifest = &verified.manifest;
        let chunk = manifest.chunks.get(index as usize).ok_or(RppError::InvalidSelection)?;
        let file = manifest
            .files
            .iter()
            .find(|file| (file.first_chunk..file.first_chunk + file.chunk_count).contains(&index))
            .ok_or(RppError::InvalidSelection)?;
        let offset = manifest.chunk_offsets[index as usize]
            - manifest.chunk_offsets.get(file.first_chunk as usize).copied().unwrap_or(0);
        let mut reader = fs::File::open(self.root.join(&file.path))?;
        reader.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0u8; chunk.len as usize];
        reader.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

/// In-memory chunks in manifest order (tests, small generated packages).
pub struct MemorySource(pub Vec<Vec<u8>>);

impl PackageSource for MemorySource {
    fn read_chunk(&self, _: &VerifiedManifest, index: u32) -> Result<Vec<u8>> {
        self.0.get(index as usize).cloned().ok_or(RppError::InvalidSelection)
    }
}
