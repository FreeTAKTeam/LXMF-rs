use super::*;

fn age(path: &Path) {
    let file = std::fs::File::options().write(true).open(path).expect("cache fixture");
    file.set_times(
        std::fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
    )
    .expect("age cache fixture");
}

#[tokio::test]
async fn announce_cleanup_is_bounded_and_preserves_active_and_tunnel_references() {
    let temp = tempfile::tempdir().expect("cache root");
    let cache = ReticulumPacketDiskCache::new(temp.path());
    let directory = temp.path().join("cache").join("announces");
    std::fs::create_dir_all(&directory).expect("announce directory");
    let hashes = (1..=5).map(|value| Hash::new([value; HASH_SIZE])).collect::<Vec<_>>();
    let paths =
        hashes.iter().map(|hash| directory.join(hex::encode(hash.as_slice()))).collect::<Vec<_>>();
    for path in &paths {
        std::fs::write(path, b"cached packet").expect("cache packet");
        age(path);
    }

    let active = [hashes[0]];
    let tunnel = [hashes[1]];
    let mut scan = None;
    let (examined, removed, _, complete) = cache
        .clean_announces_batch(&active, &tunnel, &mut scan, 2, Duration::ZERO)
        .await
        .expect("first cleanup batch");
    assert_eq!(examined, 2);
    assert!(removed <= 2);
    assert!(!complete);

    let mut totals = (examined, removed);
    loop {
        let (examined, removed, _, complete) = cache
            .clean_announces_batch(&active, &tunnel, &mut scan, 2, Duration::ZERO)
            .await
            .expect("next cleanup batch");
        assert!(examined <= 2);
        totals.0 += examined;
        totals.1 += removed;
        if complete {
            break;
        }
    }
    assert_eq!(totals, (5, 3));
    assert!(paths[0].exists(), "active route packet remains on disk");
    assert!(paths[1].exists(), "tunnel route packet remains on disk");
    assert_eq!(paths.iter().filter(|path| path.exists()).count(), 2);
}

#[tokio::test]
async fn announce_cleanup_grace_protects_recent_unreferenced_files() {
    let temp = tempfile::tempdir().expect("cache root");
    let cache = ReticulumPacketDiskCache::new(temp.path());
    let hash = Hash::new([9; HASH_SIZE]);
    let path = cache.path(hash, true);
    std::fs::create_dir_all(path.parent().expect("announce directory"))
        .expect("announce directory");
    std::fs::write(&path, b"recent packet").expect("cache packet");

    let mut scan = None;
    let (examined, removed, _, complete) = cache
        .clean_announces_batch(&[], &[], &mut scan, 1, Duration::from_secs(24 * 60 * 60))
        .await
        .expect("cleanup with grace period");
    assert_eq!((examined, removed, complete), (1, 0, false));
    assert!(path.exists(), "recent packet is protected by the grace period");
    assert_eq!(
        cache
            .clean_announces_batch(&[], &[], &mut scan, 1, Duration::from_secs(24 * 60 * 60))
            .await
            .expect("finish cleanup scan"),
        (0, 0, 0, true)
    );
}

#[tokio::test]
async fn announce_cleanup_fingerprint_map_matches_hash_set_membership() {
    let hashes = (1..=5).map(|value| Hash::new([value; HASH_SIZE])).collect::<Vec<_>>();
    let retained = HashSet::from([hashes[0], hashes[1]]);
    let mut fingerprints = HashMap::from([(hashes[0], [1; 32]), (hashes[1], [2; 32])]);
    // A repeated packet on another interface replaces its fingerprint, not
    // membership. Cleanup must consult only the final map's keys.
    fingerprints.insert(hashes[1], [3; 32]);
    let mut survivors = Vec::new();
    for map in [false, true] {
        let root = tempfile::tempdir().expect("cache root");
        let cache = ReticulumPacketDiskCache::new(root.path());
        let dir = root.path().join("cache/announces");
        std::fs::create_dir_all(&dir).expect("cache directory");
        for hash in &hashes {
            let path = dir.join(hex::encode(hash.as_slice()));
            std::fs::write(&path, b"cached").expect("cache entry");
            age(&path);
        }
        let mut scan = None;
        let mut totals = (0, 0, 0);
        loop {
            let batch = if map {
                cache
                    .clean_announces_batch_for_fingerprints(
                        &fingerprints,
                        &mut scan,
                        2,
                        Duration::ZERO,
                    )
                    .await
            } else {
                cache
                    .clean_announces_batch_for_hashes(&retained, &mut scan, 2, Duration::ZERO)
                    .await
            }
            .expect("bounded cleanup");
            assert!(batch.0 <= 2);
            totals.0 += batch.0;
            totals.1 += batch.1;
            totals.2 += batch.2;
            if batch.3 {
                break;
            }
        }
        assert_eq!(totals, (5, 3, 18));
        survivors.push(
            hashes
                .iter()
                .filter(|hash| dir.join(hex::encode(hash.as_slice())).exists())
                .copied()
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(survivors[0], survivors[1]);
    assert_eq!(survivors[0], [hashes[0], hashes[1]]);
}
