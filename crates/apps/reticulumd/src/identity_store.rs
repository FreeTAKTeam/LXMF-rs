use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rand_core::OsRng;

use rns_core::identity::PrivateIdentity;

pub fn load_or_create_identity(path: &Path) -> io::Result<PrivateIdentity> {
    match fs::read(path) {
        Ok(bytes) => {
            return PrivateIdentity::from_private_key_bytes(&bytes).map_err(|err| {
                io::Error::new(io::ErrorKind::InvalidData, format!("invalid identity: {err:?}"))
            });
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }

    let identity = PrivateIdentity::new_from_rand(OsRng);
    write_identity_file(path, &identity.to_private_key_bytes())?;
    Ok(identity)
}

fn write_identity_file(path: &Path, key_bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let tmp_path = path.with_extension(format!("tmp-{unique}"));
    write_private_key_tmp(&tmp_path, key_bytes)?;

    #[cfg(windows)]
    if path.exists() {
        let _ = fs::remove_file(path);
    }

    fs::rename(&tmp_path, path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

fn write_private_key_tmp(path: &Path, key_bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = options.open(path)?;
        file.write_all(key_bytes)?;
        file.sync_all()?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        use std::fs::OpenOptions;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options.open(path)?;
        file.write_all(key_bytes)?;
        file.sync_all()?;
        Ok(())
    }
}
