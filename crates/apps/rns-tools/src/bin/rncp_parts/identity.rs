use crate::Cli;
use rand_core::OsRng;
use rns_transport::identity::PrivateIdentity;
use std::fs;
use std::io;

pub(crate) fn load(cli: &Cli) -> io::Result<PrivateIdentity> {
    let identity = if let Some(path) = cli.identity.as_deref() {
        if path.exists() {
            let bytes = fs::read(path)?;
            PrivateIdentity::from_private_key_bytes(&bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid Reticulum identity")
            })?
        } else {
            create(cli)
        }
    } else {
        create(cli)
    };

    if let Some(path) = cli.identity.as_deref() {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, identity.to_private_key_bytes())?;
        }
    }

    Ok(identity)
}

fn create(cli: &Cli) -> PrivateIdentity {
    cli.identity_seed
        .as_deref()
        .map(PrivateIdentity::new_from_name)
        .unwrap_or_else(|| PrivateIdentity::new_from_rand(OsRng))
}
