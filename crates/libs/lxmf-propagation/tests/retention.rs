use lxmf_propagation::{Error, OwnedStore, StoreLimits, RETENTION_SECS};
use rns_core::identity::PrivateIdentity;

#[test]
fn reopening_cannot_shorten_or_extend_the_original_deduplication_policy() {
    let dir = tempfile::tempdir().expect("test directory");
    let path = dir.path().join("custody.db");
    let sender = PrivateIdentity::new_from_name("retention-owner");
    let original = StoreLimits { retention_secs: RETENTION_SECS, capacity_bytes: 10_000 };
    drop(OwnedStore::open(&path, sender.clone(), original).expect("original policy"));
    let shorter = StoreLimits { retention_secs: 100, ..original };
    assert!(matches!(
        OwnedStore::open(&path, sender.clone(), shorter),
        Err(Error::InvalidState(_))
    ));
    drop(
        OwnedStore::open(&path, sender.clone(), original).expect("unchanged policy remains usable"),
    );
    let short_path = dir.path().join("short-custody.db");
    drop(OwnedStore::open(&short_path, sender.clone(), shorter).expect("short initial policy"));
    assert!(matches!(
        OwnedStore::open(&short_path, sender.clone(), original),
        Err(Error::InvalidState(_))
    ));
    OwnedStore::open(&short_path, sender, shorter).expect("rejected reopen did not alter policy");
}
