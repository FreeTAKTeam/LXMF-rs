use super::*;

fn packet(index: u64) -> Packet {
    let mut packet = Packet::default();
    packet.header.packet_type = PacketType::Announce;
    packet.data = crate::packet::PacketDataBuffer::new_from_slice(&index.to_be_bytes());
    packet
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "explicit repeated-write qualification on a real disk filesystem"]
async fn profile_repeated_announce_cache_writes() {
    fn io() -> (u64, u64) {
        let data = std::fs::read_to_string("/proc/self/io").expect("process IO");
        let field = |name: &str| {
            data.lines()
                .find_map(|line| line.strip_prefix(name))
                .expect("field")
                .trim()
                .parse::<u64>()
                .expect("counter")
        };
        (field("syscw:"), field("write_bytes:"))
    }
    let temp = tempfile::tempdir().expect("cache");
    let cache = ReticulumAnnounceCache::new(temp.path());
    cache.ensure_dir().await.expect("directory");
    let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"write-probe-interface"));
    let count = 10_000_u64;
    for index in 0..count {
        let packet = packet(index);
        cache.write(packet.hash(), iface, packet).await.expect("initial cache");
    }
    let before = io();
    let started = std::time::Instant::now();
    for index in 0..count {
        let packet = packet(index);
        cache.write(packet.hash(), iface, packet).await.expect("unchanged cache");
    }
    let elapsed = started.elapsed();
    let after = io();
    println!(
        "cache_rows={count} repeated_write_syscalls={} repeated_write_bytes={} elapsed_ms={}",
        after.0 - before.0,
        after.1 - before.1,
        elapsed.as_secs_f64() * 1000.0
    );
}

fn stamp(path: &Path) -> std::time::SystemTime {
    let file = std::fs::File::options().write(true).open(path).expect("cache file");
    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(946_684_800);
    file.set_times(std::fs::FileTimes::new().set_modified(old)).expect("sentinel timestamp");
    file.metadata().expect("metadata").modified().expect("modified")
}

#[tokio::test]
async fn unchanged_cached_announce_preserves_file_and_changed_interface_is_persisted() {
    let temp = tempfile::tempdir().expect("cache");
    let cache = ReticulumAnnounceCache::new(temp.path());
    cache.ensure_dir().await.expect("directory");
    let packet = packet(1);
    let hash = packet.hash();
    let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"first-interface"));
    cache.write(hash, iface, packet.clone()).await.expect("initial write");
    let path = cache.path(hash);
    let before = std::fs::read(&path).expect("initial bytes");
    let modified = stamp(&path);
    cache.write(hash, iface, packet.clone()).await.expect("unchanged write");
    assert_eq!(std::fs::read(&path).expect("unchanged bytes"), before);
    assert_eq!(std::fs::metadata(&path).expect("metadata").modified().expect("modified"), modified);
    let next_iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"second-interface"));
    cache.write(hash, next_iface, packet.clone()).await.expect("changed interface");
    assert_eq!(
        std::fs::read(&path).expect("changed bytes"),
        encode_cached_announce(next_iface, packet).expect("encoding")
    );
    assert_ne!(std::fs::metadata(&path).expect("metadata").modified().expect("modified"), modified);
}

#[tokio::test]
async fn cached_announce_replaces_corrupt_content_and_propagates_file_errors() {
    let temp = tempfile::tempdir().expect("cache");
    let cache = ReticulumAnnounceCache::new(temp.path());
    cache.ensure_dir().await.expect("directory");
    let packet = packet(2);
    let hash = packet.hash();
    let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"repair-interface"));
    let path = cache.path(hash);
    std::fs::write(&path, vec![0xc1; 1_000_000]).expect("oversized corrupt cache");
    cache.write(hash, iface, packet.clone()).await.expect("repair");
    assert_eq!(
        std::fs::read(&path).expect("repaired bytes"),
        encode_cached_announce(iface, packet.clone()).expect("encoding")
    );
    std::fs::remove_file(&path).expect("remove fixture");
    std::fs::create_dir(&path).expect("invalid file fixture");
    assert!(cache.write(hash, iface, packet).await.is_err());
}

#[tokio::test]
async fn cached_announce_persists_mutable_headers_even_when_hash_is_unchanged() {
    let temp = tempfile::tempdir().expect("cache");
    let cache = ReticulumAnnounceCache::new(temp.path());
    cache.ensure_dir().await.expect("directory");
    let mut packet = packet(3);
    let hash = packet.hash();
    let iface = AddressHash::new_from_hash(&Hash::new_from_slice(b"header-interface"));
    cache.write(hash, iface, packet.clone()).await.expect("initial");
    packet.header.hops += 1;
    assert_eq!(packet.hash(), hash, "hash excludes mutable hop count");
    cache.write(hash, iface, packet.clone()).await.expect("new hops");
    assert_eq!(
        std::fs::read(cache.path(hash)).expect("bytes"),
        encode_cached_announce(iface, packet).expect("encoding")
    );
}
