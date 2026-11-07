//! Encode the existing Python snapshot arrays without an owned MessagePack DOM.
use crate::error::RnsError;
use crate::hash::Hash;
use crate::transport::path_table::{PythonPathEntry, RandomBlob, MAX_RANDOM_BLOBS};
use crate::transport::tunnels::{PythonTunnelEntry, PythonTunnelPathEntry};

#[derive(Default)]
struct Encoder(Vec<u8>);

impl Encoder {
    fn array(&mut self, len: usize) -> Result<(), RnsError> {
        let len = u32::try_from(len).map_err(|_| RnsError::InvalidArgument)?;
        rmp::encode::write_array_len(&mut self.0, len).map_err(|_| RnsError::InvalidArgument)?;
        Ok(())
    }
    fn binary(&mut self, bytes: &[u8]) -> Result<(), RnsError> {
        rmp::encode::write_bin(&mut self.0, bytes).map_err(|_| RnsError::InvalidArgument)
    }
    fn float(&mut self, value: f64) -> Result<(), RnsError> {
        rmp::encode::write_f64(&mut self.0, value).map_err(|_| RnsError::InvalidArgument)
    }
    fn hops(&mut self, hops: u8) -> Result<(), RnsError> {
        rmp::encode::write_uint(&mut self.0, u64::from(hops))
            .map_err(|_| RnsError::InvalidArgument)?;
        Ok(())
    }
    fn blobs(&mut self, blobs: &[RandomBlob]) -> Result<(), RnsError> {
        self.array(blobs.len())?;
        for blob in blobs {
            self.binary(blob)?;
        }
        Ok(())
    }
    fn optional_hash(&mut self, hash: Option<Hash>) -> Result<(), RnsError> {
        match hash {
            Some(hash) => self.binary(hash.as_slice()),
            None => rmp::encode::write_nil(&mut self.0).map_err(|_| RnsError::InvalidArgument),
        }
    }
    fn path(&mut self, entry: &PythonPathEntry) -> Result<(), RnsError> {
        self.array(8)?;
        self.binary(entry.destination.as_slice())?;
        self.float(entry.timestamp_secs)?;
        self.binary(entry.received_from.as_slice())?;
        self.hops(entry.hops)?;
        self.float(entry.expires_secs)?;
        // Retain exactly the active-path codec's existing newest-64 policy.
        self.blobs(
            &entry.random_blobs[entry.random_blobs.len().saturating_sub(MAX_RANDOM_BLOBS)..],
        )?;
        self.binary(entry.interface_hash.as_slice())?;
        self.binary(entry.packet_hash.as_slice())
    }
    fn tunnel_path(&mut self, entry: &PythonTunnelPathEntry) -> Result<(), RnsError> {
        self.array(8)?;
        self.binary(entry.destination.as_slice())?;
        self.float(entry.timestamp_secs)?;
        self.binary(entry.received_from.as_slice())?;
        self.hops(entry.hops)?;
        self.float(entry.expires_secs)?;
        // Tunnel encoding historically preserves every supplied random blob.
        self.blobs(&entry.random_blobs)?;
        self.optional_hash(entry.interface_hash)?;
        self.binary(entry.packet_hash.as_slice())
    }
}

pub(in crate::transport) fn encode_paths(entries: &[PythonPathEntry]) -> Result<Vec<u8>, RnsError> {
    let mut encoder = Encoder::default();
    encoder.array(entries.len())?;
    for entry in entries {
        encoder.path(entry)?;
    }
    Ok(encoder.0)
}

pub(in crate::transport) fn encode_tunnels(
    entries: &[PythonTunnelEntry],
) -> Result<Vec<u8>, RnsError> {
    let mut encoder = Encoder::default();
    encoder.array(entries.len())?;
    for entry in entries {
        encoder.array(4)?;
        encoder.binary(entry.tunnel_id.as_slice())?;
        encoder.optional_hash(entry.interface_hash)?;
        encoder.array(entry.paths.len())?;
        for path in &entry.paths {
            encoder.tunnel_path(path)?;
        }
        encoder.float(entry.expires_secs)?;
    }
    Ok(encoder.0)
}

#[cfg(test)]
#[path = "python_snapshot_codec/tests.rs"]
mod tests;
