use rns_core::key_manager::{KeyPurpose, StoredKey};

#[cfg(unix)]
use rns_core::key_manager::{FileKeyManager, KeyManagerBackend};

fn sample_key(key_id: &str) -> StoredKey {
    StoredKey {
        key_id: key_id.to_owned(),
        purpose: KeyPurpose::IdentitySigning,
        material: vec![1, 2, 3, 4],
    }
}

#[test]
fn stored_key_debug_redacts_material() {
    let formatted = format!("{:?}", sample_key("debug-key"));
    assert!(formatted.contains("debug-key"));
    assert!(formatted.contains("[REDACTED]"));
    assert!(!formatted.contains("[1, 2, 3, 4]"));
}

#[cfg(unix)]
#[test]
fn file_key_manager_uses_private_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("keys");
    std::fs::create_dir(&root).expect("create permissive key directory");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755))
        .expect("set permissive directory mode");
    let manager = FileKeyManager::new(&root).expect("file manager");

    manager.put(sample_key("private-key")).expect("store private key");

    let directory_mode =
        std::fs::metadata(&root).expect("key directory metadata").permissions().mode() & 0o777;
    let file_mode = std::fs::metadata(root.join("private-key.key"))
        .expect("key file metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(directory_mode, 0o700);
    assert_eq!(file_mode, 0o600);
}

#[cfg(unix)]
#[test]
fn file_key_manager_does_not_follow_legacy_temp_symlink() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let victim = temp.path().join("victim");
    std::fs::write(&victim, b"unchanged").expect("write victim");
    let root = temp.path().join("keys");
    let manager = FileKeyManager::new(&root).expect("file manager");
    let legacy_tmp = root.join("node-signing.tmp");
    symlink(&victim, &legacy_tmp).expect("create legacy temp symlink");

    manager.put(sample_key("node-signing")).expect("store key");

    assert_eq!(std::fs::read(&victim).expect("read victim"), b"unchanged");
    assert!(legacy_tmp.is_symlink());
    assert!(manager.get("node-signing").expect("load key").is_some());
}
