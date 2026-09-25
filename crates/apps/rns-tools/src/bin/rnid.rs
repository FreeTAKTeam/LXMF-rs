use clap::{Parser, Subcommand};
use rand_core::OsRng;
use rns_transport::identity::{lxmf_verify, Identity, PrivateIdentity};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::io::Cursor;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "rnid", about = "Create and inspect Reticulum identities")]
struct Cli {
    #[arg(short = 'i', long, global = true, value_name = "PATH")]
    identity: Option<PathBuf>,
    #[arg(short = 's', long, global = true, value_name = "PATH", num_args = 1..)]
    sign: Option<Vec<PathBuf>>,
    #[arg(short = 'V', long, global = true, value_name = "PATH")]
    validate: Option<PathBuf>,
    #[arg(short = 'f', long, global = true)]
    force: bool,
    #[command(subcommand)]
    command: Option<IdentityCommand>,
}

#[derive(Debug, Subcommand)]
enum IdentityCommand {
    Generate {
        #[arg(long)]
        output: PathBuf,
    },
    Show {
        identity: PathBuf,
        #[arg(long)]
        private: bool,
    },
    Sign {
        identity: PathBuf,
        input: PathBuf,
    },
}

fn main() -> std::process::ExitCode {
    match run(Cli::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            if error.kind() == io::ErrorKind::AlreadyExists {
                println!("{error}");
                std::process::ExitCode::from(11)
            } else if error.kind() == io::ErrorKind::InvalidData
                && error.to_string() == "invalid signature"
            {
                eprintln!("rnid: {error}");
                std::process::ExitCode::from(10)
            } else {
                eprintln!("rnid: {error}");
                std::process::ExitCode::FAILURE
            }
        }
    }
}

fn run(cli: Cli) -> io::Result<()> {
    let Cli { identity, sign, validate, force, command } = cli;
    if let Some(validation_target) = validate {
        if sign.is_some() || command.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--validate cannot be combined with --sign or a subcommand",
            ));
        }
        let (input_path, signature_path) = validation_paths(&validation_target);
        if validate_signature(identity.as_deref(), &input_path, &signature_path)? {
            println!(
                "Signature {} for file {} is valid",
                signature_path.display(),
                input_path.display()
            );
            return Ok(());
        }
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid signature"));
    }
    if let Some(inputs) = sign {
        if command.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--sign cannot be combined with a subcommand",
            ));
        }
        let identity = identity.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "--sign needs --identity")
        })?;
        for input in inputs {
            sign_file(&identity, &input, force)?;
        }
        return Ok(());
    }
    if identity.is_some() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "--identity needs --sign"));
    }

    match command {
        Some(IdentityCommand::Generate { output }) => {
            if output.exists() && !force {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "identity exists; use --force",
                ));
            }
            let identity = PrivateIdentity::new_from_rand(OsRng);
            write_private_identity(&output, &identity)?;
            println!("{}", identity.address_hash());
        }
        Some(IdentityCommand::Show { identity, private }) => {
            let identity = read_private_identity(&identity)?;
            if private {
                println!("{}", hex::encode(identity.to_private_key_bytes()));
            } else {
                println!("{}", identity.address_hash());
            }
        }
        Some(IdentityCommand::Sign { identity, input }) => sign_file(&identity, &input, force)?,
        None => {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a command is required"));
        }
    }
    Ok(())
}

fn validation_paths(target: &Path) -> (PathBuf, PathBuf) {
    if target.extension() == Some(OsStr::new("rsg")) {
        (target.with_extension(""), target.to_path_buf())
    } else {
        let mut signature_path = target.as_os_str().to_os_string();
        signature_path.push(".rsg");
        (target.to_path_buf(), PathBuf::from(signature_path))
    }
}

fn validate_signature(
    required_identity_path: Option<&Path>,
    input_path: &Path,
    signature_path: &Path,
) -> io::Result<bool> {
    let input = fs::read(input_path)?;
    let signature = fs::read(signature_path)?;
    let required_identity = required_identity_path.map(read_private_identity).transpose()?;

    if signature.len() == 64 {
        return Ok(required_identity
            .as_ref()
            .is_some_and(|identity| lxmf_verify(identity.as_identity(), &input, &signature)));
    }
    if signature.len() < 65 {
        return Ok(false);
    }

    let (signature_bytes, envelope_bytes) = signature.split_at(64);
    let mut envelope_cursor = Cursor::new(envelope_bytes);
    let Ok(envelope) = rmpv::decode::read_value(&mut envelope_cursor) else { return Ok(false) };
    let rmpv::Value::Map(envelope_map) = envelope else { return Ok(false) };
    let envelope = rmpv::Value::Map(envelope_map);
    if envelope_cursor.position() != envelope_bytes.len() as u64
        || envelope["hashtype"].as_str() != Some("sha256")
    {
        return Ok(false);
    }
    let rmpv::Value::Binary(expected_hash) = &envelope["hash"] else {
        return Ok(false);
    };
    if expected_hash.as_slice() != Sha256::digest(&input).as_slice() {
        return Ok(false);
    }
    let rmpv::Value::Map(metadata) = &envelope["meta"] else {
        return Ok(false);
    };
    let metadata = rmpv::Value::Map(metadata.clone());
    let rmpv::Value::Binary(signer_hash) = &metadata["signer"] else {
        return Ok(false);
    };
    let rmpv::Value::Binary(public_key) = &metadata["pubkey"] else {
        return Ok(false);
    };
    if public_key.len() != 64 {
        return Ok(false);
    }
    let Ok(signer) = Identity::try_new_from_slices(&public_key[..32], &public_key[32..]) else {
        return Ok(false);
    };
    if signer_hash.as_slice() != signer.address_hash.as_slice()
        || required_identity
            .as_ref()
            .is_some_and(|identity| identity.address_hash() != &signer.address_hash)
    {
        return Ok(false);
    }
    Ok(lxmf_verify(&signer, envelope_bytes, signature_bytes))
}

fn sign_file(identity_path: &Path, input: &Path, force: bool) -> io::Result<()> {
    let signer = read_private_identity(identity_path)?;
    let message = fs::read(input)?;
    let signature_path = signature_path(input);
    if signature_path.exists() && !force {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "The signature file \"{}\" already exists, not overwriting",
                signature_path.display()
            ),
        ));
    }
    fs::write(&signature_path, create_signature(&signer, &message)?)?;
    println!("Signed file {} with {}", input.display(), signer.address_hash());
    Ok(())
}

fn signature_path(input: &Path) -> PathBuf {
    let mut path = input.as_os_str().to_os_string();
    path.push(".rsg");
    PathBuf::from(path)
}

fn create_signature(identity: &PrivateIdentity, message: &[u8]) -> io::Result<Vec<u8>> {
    let digest = Sha256::digest(message);
    let signed_data = rmpv::Value::Map(vec![
        (rmpv::Value::from("hashtype"), rmpv::Value::from("sha256")),
        (rmpv::Value::from("hash"), rmpv::Value::Binary(digest.to_vec())),
        (
            rmpv::Value::from("meta"),
            rmpv::Value::Map(vec![
                (
                    rmpv::Value::from("signer"),
                    rmpv::Value::Binary(identity.address_hash().as_slice().to_vec()),
                ),
                (
                    rmpv::Value::from("pubkey"),
                    rmpv::Value::Binary(
                        [
                            identity.as_identity().public_key_bytes().as_slice(),
                            identity.as_identity().verifying_key_bytes().as_slice(),
                        ]
                        .concat(),
                    ),
                ),
            ]),
        ),
    ]);
    let mut envelope = Vec::new();
    rmpv::encode::write_value(&mut envelope, &signed_data).map_err(|error| {
        io::Error::other(format!("could not encode signature envelope: {error}"))
    })?;
    let signature = identity.sign(&envelope);
    let mut output = Vec::with_capacity(signature.to_bytes().len() + envelope.len());
    output.extend_from_slice(&signature.to_bytes());
    output.extend_from_slice(&envelope);
    Ok(output)
}

fn write_private_identity(path: &Path, identity: &PrivateIdentity) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, identity.to_private_key_bytes())
}

fn read_private_identity(path: &Path) -> io::Result<PrivateIdentity> {
    let bytes = fs::read(path)?;
    PrivateIdentity::from_private_key_bytes(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Reticulum identity"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_file_roundtrips_private_material() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("identity");
        let identity = PrivateIdentity::new_from_rand(OsRng);
        write_private_identity(&path, &identity).expect("write");
        let restored = read_private_identity(&path).expect("read");
        assert_eq!(restored.to_private_key_bytes(), identity.to_private_key_bytes());
    }

    #[test]
    fn signature_envelope_uses_python_rsg_sha256_contract() {
        let identity = PrivateIdentity::new_from_name("rnid-sign-test");
        let signature = create_signature(&identity, b"binary\0payload").expect("signature");
        assert!(signature.len() > 64);
        let envelope = rmpv::decode::read_value(&mut &signature[64..]).expect("RSG envelope");
        assert_eq!(envelope["hashtype"], rmpv::Value::from("sha256"));
        assert_eq!(
            envelope["hash"],
            rmpv::Value::Binary(Sha256::digest(b"binary\0payload").to_vec())
        );
        let rmpv::Value::Map(meta) = &envelope["meta"] else { panic!("RSG metadata map") };
        assert_eq!(meta[0].0, rmpv::Value::from("signer"));
        assert_eq!(meta[1].0, rmpv::Value::from("pubkey"));
    }

    #[test]
    fn rust_sign_subcommand_remains_available_alongside_reference_flags() {
        let cli = Cli::try_parse_from(["rnid", "sign", "identity.rid", "payload.bin"])
            .expect("sign subcommand");
        assert!(matches!(cli.command, Some(IdentityCommand::Sign { .. })));
    }
}
