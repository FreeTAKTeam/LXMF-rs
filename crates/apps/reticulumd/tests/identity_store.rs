use std::fs;
use std::io;

use reticulum_daemon::identity_store::load_or_create_identity;

#[test]
fn identity_persists_across_reloads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("identity.bin");

    let first = load_or_create_identity(&path).expect("create identity");
    assert!(path.exists(), "identity file should be created");

    let second = load_or_create_identity(&path).expect("load identity");
    assert_eq!(
        first.to_private_key_bytes(),
        second.to_private_key_bytes(),
        "identity should be stable across reloads"
    );
}

#[test]
fn identity_load_returns_error_on_unreadable_existing_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("identity-dir");
    fs::create_dir(&path).expect("create directory");

    let err = match load_or_create_identity(&path) {
        Ok(_) => panic!("directory read should fail"),
        Err(err) => err,
    };
    assert_ne!(
        err.kind(),
        io::ErrorKind::NotFound,
        "existing unreadable paths should not be treated as missing identity files"
    );
    assert!(path.is_dir(), "identity path should remain intact when read fails");
}

#[cfg(unix)]
#[test]
fn identity_file_permissions_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("identity.bin");
    load_or_create_identity(&path).expect("create identity");

    let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "identity file mode should be 0600");
}
