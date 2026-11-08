//! Encode the existing Python snapshot arrays without an owned MessagePack DOM.
use crate::error::RnsError;
use crate::hash::Hash;
use crate::transport::path_table::{PythonPathEntry, RandomBlob, MAX_RANDOM_BLOBS};
use crate::transport::tunnels::{PythonTunnelEntry, PythonTunnelPathEntry};
use std::io::{self, Write};

#[derive(Default)]
struct Encoder<W = Vec<u8>>(W);

#[derive(Default)]
struct ByteCount {
    bytes: usize,
}

impl Write for ByteCount {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self.bytes.checked_add(bytes.len()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Python snapshot length overflow")
        })?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Encoder<Vec<u8>> {
    fn reserved(len: usize) -> Result<Self, RnsError> {
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(len).map_err(|_| RnsError::OutOfMemory)?;
        Ok(Self(bytes))
    }
    fn finish(self, expected: usize) -> Result<Vec<u8>, RnsError> {
        if self.0.len() != expected {
            return Err(RnsError::InvalidArgument);
        }
        Ok(self.0)
    }
}

impl<W: Write> Encoder<W> {
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
    // The exact same writer counts without allocation, then emits into one
    // reserved output buffer. Both passes borrow the immutable snapshot and run
    // after the transport/interface locks have been released.
    let mut count = Encoder(ByteCount::default());
    write_paths(&mut count, entries)?;
    let mut encoder = Encoder::reserved(count.0.bytes)?;
    write_paths(&mut encoder, entries)?;
    encoder.finish(count.0.bytes)
}

fn write_paths<W: Write>(
    encoder: &mut Encoder<W>,
    entries: &[PythonPathEntry],
) -> Result<(), RnsError> {
    encoder.array(entries.len())?;
    for entry in entries {
        encoder.path(entry)?;
    }
    Ok(())
}

pub(in crate::transport) fn encode_tunnels(
    entries: &[PythonTunnelEntry],
) -> Result<Vec<u8>, RnsError> {
    let mut count = Encoder(ByteCount::default());
    write_tunnels(&mut count, entries)?;
    let mut encoder = Encoder::reserved(count.0.bytes)?;
    write_tunnels(&mut encoder, entries)?;
    encoder.finish(count.0.bytes)
}

fn write_tunnels<W: Write>(
    encoder: &mut Encoder<W>,
    entries: &[PythonTunnelEntry],
) -> Result<(), RnsError> {
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
    Ok(())
}

#[cfg(test)]
#[path = "python_snapshot_codec/tests.rs"]
mod tests;
