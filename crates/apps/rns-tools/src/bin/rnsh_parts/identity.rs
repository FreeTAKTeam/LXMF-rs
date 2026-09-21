use rand_core::OsRng;
use rns_transport::identity::PrivateIdentity;
use std::fs;
use std::io;
use std::path::Path;

pub(crate) fn load(path: Option<&Path>, seed: Option<&str>) -> io::Result<PrivateIdentity> {
    let identity = if let Some(path) = path {
        if path.is_file() {
            let bytes = fs::read(path)?;
            PrivateIdentity::from_private_key_bytes(&bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid Reticulum identity")
            })?
        } else if path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "rnsh identity path is not a regular file",
            ));
        } else {
            create(seed)
        }
    } else {
        create(seed)
    };

    if let Some(path) = path {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, identity.to_private_key_bytes())?;
        }
    }

    Ok(identity)
}

fn create(seed: Option<&str>) -> PrivateIdentity {
    seed.map(PrivateIdentity::new_from_name)
        .unwrap_or_else(|| PrivateIdentity::new_from_rand(OsRng))
}
