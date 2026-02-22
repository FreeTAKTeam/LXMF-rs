use crate::LxmfError;
use rand_core::OsRng;
use reticulum::identity::PrivateIdentity;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn drop_empty_identity_stub(path: &Path) -> Result<(), LxmfError> {
    if let Ok(meta) = fs::metadata(path) {
        if meta.is_file() && meta.len() == 0 {
            fs::remove_file(path).map_err(|err| LxmfError::Io(err.to_string()))?;
        }
    }
    Ok(())
}

pub(super) fn load_or_create_identity(path: &Path) -> Result<PrivateIdentity, LxmfError> {
    match fs::read(path) {
        Ok(bytes) => {
            return PrivateIdentity::from_private_key_bytes(&bytes)
                .map_err(|err| LxmfError::Io(format!("invalid identity: {err:?}")));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(LxmfError::Io(err.to_string())),
    }

    let identity = PrivateIdentity::new_from_rand(OsRng);
    write_identity_file(path, &identity.to_private_key_bytes())?;
    Ok(identity)
}

fn write_identity_file(path: &Path, key_bytes: &[u8]) -> Result<(), LxmfError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| LxmfError::Io(err.to_string()))?;
        }
    }

    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let tmp_path = path.with_extension(format!("tmp-{unique}"));
    write_private_key_tmp(&tmp_path, key_bytes)?;

    #[cfg(windows)]
    if path.exists() {
        let _ = fs::remove_file(path);
    }

    fs::rename(&tmp_path, path).map_err(|err| LxmfError::Io(err.to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|err| LxmfError::Io(err.to_string()))?;
    }

    Ok(())
}

fn write_private_key_tmp(path: &Path, key_bytes: &[u8]) -> Result<(), LxmfError> {
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = options.open(path).map_err(|err| LxmfError::Io(err.to_string()))?;
        file.write_all(key_bytes).map_err(|err| LxmfError::Io(err.to_string()))?;
        file.sync_all().map_err(|err| LxmfError::Io(err.to_string()))?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        use std::fs::OpenOptions;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options.open(path).map_err(|err| LxmfError::Io(err.to_string()))?;
        file.write_all(key_bytes).map_err(|err| LxmfError::Io(err.to_string()))?;
        file.sync_all().map_err(|err| LxmfError::Io(err.to_string()))?;
        Ok(())
    }
}
