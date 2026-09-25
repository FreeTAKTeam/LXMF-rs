use super::{san_ref, ReticulumGitClient};
use std::io::{Read, Write};
use std::process::Command;

fn run_git_push(
    cli: &Cli,
    remote: &str,
    local_ref: &str,
    remote_ref: &str,
    force: bool,
) -> io::Result<()> {
    if cli.connect.len() != 1 || !cli.listen.is_empty() || cli.print_identity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "rngit push requires exactly one --connect endpoint and no --listen",
        ));
    }
    if san_ref(local_ref).is_none() || san_ref(remote_ref).is_none() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid Git reference"));
    }
    let root = cli.root.canonicalize()?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "--verify", local_ref])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("local Git reference not found: {local_ref}"),
        ));
    }
    let bundle_dir = tempfile::Builder::new().prefix("rngit-push-").tempdir()?;
    let bundle_path = bundle_dir.path().join("push.bundle");
    let bundled = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["bundle", "create"])
        .arg(&bundle_path)
        .arg(local_ref)
        .output()?;
    if !bundled.status.success() {
        return Err(io::Error::other(format!(
            "Git bundle creation failed: {}",
            String::from_utf8_lossy(&bundled.stderr).trim()
        )));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(bundle_path)?.read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        return Err(io::Error::other("Git created an empty push bundle"));
    }
    let mut client = ReticulumGitClient::default();
    client.attach_native_tcp(load_identity(cli)?, cli.connect[0].clone());
    let response = client
        .process_push_queue(remote, local_ref, remote_ref, &bytes, force)
        .map_err(io::Error::other)?;
    if response.first().copied() != Some(0) {
        let detail =
            String::from_utf8_lossy(response.get(1..).unwrap_or_default()).trim().to_owned();
        return Err(io::Error::other(if detail.is_empty() {
            "remote Git push failed".to_string()
        } else {
            format!("remote Git push failed: {detail}")
        }));
    }
    println!(
        "Pushed {local_ref} to {remote_ref} at {}",
        String::from_utf8_lossy(&output.stdout).trim()
    );
    Ok(())
}

fn run_git_fetch(
    cli: &Cli,
    remote: &str,
    reference: &str,
    destination_ref: &str,
) -> io::Result<()> {
    if cli.connect.len() != 1 || !cli.listen.is_empty() || cli.print_identity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "rngit fetch requires exactly one --connect endpoint and no --listen",
        ));
    }
    if san_ref(reference).is_none() || san_ref(destination_ref).is_none() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid Git reference"));
    }

    let root = cli.root.canonicalize()?;
    let mut client = ReticulumGitClient::default();
    client.attach_native_tcp(load_identity(cli)?, cli.connect[0].clone());

    let remote_refs = client.handle_git_list(remote, false).map_err(io::Error::other)?;
    let remote_sha = remote_refs
        .iter()
        .find_map(|line| {
            let (sha, listed_ref) = line.split_once(' ')?;
            (listed_ref == reference).then_some(sha.to_owned())
        })
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "remote Git reference was not found")
        })?;

    let response =
        client.process_fetch_queue(remote, &[reference.to_owned()]).map_err(io::Error::other)?;
    if response.first().copied() != Some(0) {
        let detail =
            String::from_utf8_lossy(response.get(1..).unwrap_or_default()).trim().to_owned();
        return Err(io::Error::other(if detail.is_empty() {
            "remote Git fetch failed".to_string()
        } else {
            format!("remote Git fetch failed: {detail}")
        }));
    }

    if response.len() == 1 {
        let local = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-parse", "--verify", destination_ref])
            .output()?;
        if local.status.success() && String::from_utf8_lossy(&local.stdout).trim() == remote_sha {
            println!("Already up to date: {destination_ref}");
            return Ok(());
        }
        return Err(io::Error::other(
            "remote returned an empty bundle but the destination ref is not current",
        ));
    }

    let mut bundle =
        tempfile::Builder::new().prefix("rngit-fetch-").suffix(".bundle").tempfile()?;
    bundle.write_all(&response[1..])?;
    bundle.as_file().sync_all()?;
    let bundle_path = bundle.path();
    let verification = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["bundle", "verify", "-q"])
        .arg(bundle_path)
        .output()?;
    if !verification.status.success() {
        return Err(io::Error::other(format!(
            "downloaded Git bundle failed verification: {}",
            String::from_utf8_lossy(&verification.stderr).trim()
        )));
    }

    let refspec = format!("{reference}:{destination_ref}");
    let fetched = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["fetch", "--no-tags"])
        .arg(bundle_path)
        .arg(&refspec)
        .output()?;
    if !fetched.status.success() {
        return Err(io::Error::other(format!(
            "local Git fetch failed: {}",
            String::from_utf8_lossy(&fetched.stderr).trim()
        )));
    }

    println!("Fetched {reference} to {destination_ref} from {remote}");
    Ok(())
}
