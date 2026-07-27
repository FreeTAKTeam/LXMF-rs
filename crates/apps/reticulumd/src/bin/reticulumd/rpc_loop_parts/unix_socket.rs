const RPC_UNIX_SOCKET_MODE: u32 = 0o600;
const RPC_UNIX_STAGING_DIR_MODE: u32 = 0o700;

fn bind_private_rpc_unix_listener(path: &Path) -> io::Result<UnixListener> {
    bind_private_rpc_unix_listener_with_pre_publish(path, |_| Ok(()))
}

fn bind_private_rpc_unix_listener_with_pre_publish(
    path: &Path,
    before_publish: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;

    prepare_rpc_unix_socket_path(path)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "rpc unix socket path has no file name")
    })?;
    let staging_dir = create_private_rpc_unix_staging_dir(parent, file_name)?;
    let staging_path = staging_dir.join("socket");

    let result = (|| {
        let listener = UnixListener::bind(&staging_path)?;
        std::fs::set_permissions(
            &staging_path,
            std::fs::Permissions::from_mode(RPC_UNIX_SOCKET_MODE),
        )?;
        before_publish(&staging_path)?;

        // The hard link publishes an already-private socket atomically and fails
        // instead of replacing a path created after prepare_rpc_unix_socket_path().
        std::fs::hard_link(&staging_path, path)?;
        Ok(listener)
    })();

    if staging_path.exists() {
        let _ = std::fs::remove_file(&staging_path);
    }
    if let Err(error) = std::fs::remove_dir(&staging_dir) {
        if error.kind() != io::ErrorKind::NotFound {
            log::warn!(
                "failed to remove private rpc unix staging directory {}: {error}",
                staging_dir.display()
            );
        }
    }

    result
}

fn create_private_rpc_unix_staging_dir(
    parent: &Path,
    file_name: &std::ffi::OsStr,
) -> io::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(0);

    for _ in 0..128 {
        let id = NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed);
        let staging_dir = parent.join(format!(
            ".{}.rpc-stage-{}-{id}",
            file_name.to_string_lossy(),
            std::process::id()
        ));
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(RPC_UNIX_STAGING_DIR_MODE);
        match builder.create(&staging_dir) {
            Ok(()) => {
                if let Err(error) = std::fs::set_permissions(
                    &staging_dir,
                    std::fs::Permissions::from_mode(RPC_UNIX_STAGING_DIR_MODE),
                ) {
                    let _ = std::fs::remove_dir(&staging_dir);
                    return Err(error);
                }
                return Ok(staging_dir);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "unable to create private rpc unix socket staging directory",
    ))
}

fn prepare_rpc_unix_socket_path(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::FileTypeExt;

    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => std::fs::remove_file(path),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("refusing to remove non-socket rpc unix path {}", path.display()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn cleanup_rpc_unix_socket_path(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::FileTypeExt;

    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => std::fs::remove_file(path),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
