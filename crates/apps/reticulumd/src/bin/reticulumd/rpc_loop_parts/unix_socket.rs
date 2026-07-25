const RPC_UNIX_SOCKET_MODE: u32 = 0o600;

fn bind_private_rpc_unix_listener(path: &Path) -> io::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;

    prepare_rpc_unix_socket_path(path)?;
    let listener = UnixListener::bind(path)?;
    if let Err(permission_error) = std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(RPC_UNIX_SOCKET_MODE),
    ) {
        drop(listener);
        return match cleanup_rpc_unix_socket_path(path) {
            Ok(()) => Err(io::Error::new(
                permission_error.kind(),
                format!(
                    "failed to secure rpc unix socket {}: {permission_error}",
                    path.display()
                ),
            )),
            Err(cleanup_error) => Err(io::Error::new(
                permission_error.kind(),
                format!(
                    "failed to secure rpc unix socket {}: {permission_error}; \
                     failed to remove insecure socket: {cleanup_error}",
                    path.display()
                ),
            )),
        };
    }
    Ok(listener)
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
