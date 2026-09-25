use clap::{Parser, Subcommand};
use rand_core::OsRng;
use rns_transport::identity::PrivateIdentity;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "rnid", about = "Create and inspect Reticulum identities")]
struct Cli {
    #[arg(short = 'i', long, global = true, value_name = "PATH")]
    identity: Option<PathBuf>,
    #[arg(short = 's', long, global = true, value_name = "PATH", num_args = 1..)]
    sign: Option<Vec<PathBuf>>,
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
            } else {
                eprintln!("rnid: {error}");
                std::process::ExitCode::FAILURE
            }
        }
    }
}

fn run(cli: Cli) -> io::Result<()> {
    let Cli { identity, sign, force, command } = cli;
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
