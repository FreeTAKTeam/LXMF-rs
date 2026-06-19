use super::*;
use rmpv::Value as RmpValue;
use std::io;
use std::path::{Path, PathBuf};

pub(super) struct ReticulumAnnounceCache {
    dir: PathBuf,
}

impl ReticulumAnnounceCache {
    pub(super) fn new(storage_path: &Path) -> Self {
        Self { dir: storage_path.join("cache").join("announces") }
    }

    pub(super) async fn ensure_dir(&self) -> io::Result<()> {
        tokio::fs::create_dir_all(&self.dir).await
    }

    pub(super) async fn write(
        &self,
        packet_hash: Hash,
        iface: AddressHash,
        packet: Packet,
    ) -> io::Result<()> {
        let payload = encode_cached_announce(iface, packet)?;
        tokio::fs::write(self.path(packet_hash), payload).await
    }

    pub(super) async fn restore(&self, packet_hash: Hash) -> io::Result<Option<CachedAnnounce>> {
        let Some(packet) = self.read_packet(packet_hash).await? else {
            return Ok(None);
        };
        if packet.header.packet_type != PacketType::Announce {
            return Ok(None);
        }
        let Ok(announce) = DestinationAnnounce::validate(&packet) else {
            return Ok(None);
        };
        let destination = announce.destination;
        Ok(Some(CachedAnnounce { packet, destination }))
    }

    async fn read_packet(&self, packet_hash: Hash) -> io::Result<Option<Packet>> {
        let payload = match tokio::fs::read(self.path(packet_hash)).await {
            Ok(payload) => payload,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let value: RmpValue = rmpv::decode::read_value(&mut std::io::Cursor::new(payload))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decode cached announce"))?;
        let RmpValue::Array(fields) = value else {
            return Ok(None);
        };
        let Some(raw) = fields.first().and_then(rmp_bytes) else {
            return Ok(None);
        };
        Packet::from_bytes(raw).map(Some).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "decode cached announce packet")
        })
    }

    fn path(&self, packet_hash: Hash) -> PathBuf {
        self.dir.join(hex::encode(packet_hash.as_slice()))
    }
}

pub(super) struct CachedAnnounce {
    pub(super) packet: Packet,
    pub(super) destination: SingleOutputDestination,
}

fn encode_cached_announce(iface: AddressHash, packet: Packet) -> io::Result<Vec<u8>> {
    let raw = packet
        .to_bytes()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode cached announce"))?;
    let value =
        RmpValue::Array(vec![RmpValue::Binary(raw), RmpValue::String(iface.to_string().into())]);
    let mut payload = Vec::new();
    rmpv::encode::write_value(&mut payload, &value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "encode cached announce"))?;
    Ok(payload)
}

fn rmp_bytes(value: &RmpValue) -> Option<&[u8]> {
    match value {
        RmpValue::Binary(bytes) => Some(bytes),
        RmpValue::String(text) => text.as_str().map(str::as_bytes),
        _ => None,
    }
}
