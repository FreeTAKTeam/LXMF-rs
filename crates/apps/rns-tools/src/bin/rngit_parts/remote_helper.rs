use super::{san_ref, san_sha, ReticulumGitClient};
use rns_transport::identity::PrivateIdentity;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

pub fn run() -> io::Result<()> {
    match run_protocol() {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

fn run_protocol() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let _remote_name = args.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "usage: git-remote-rns <name> <url>")
    })?;
    let remote = args.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "usage: git-remote-rns <name> <url>")
    })?;
    if args.next().is_some() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "unexpected helper argument"));
    }
    let endpoint = std::env::var("RNGIT_CONNECT")
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "RNGIT_CONNECT is required"))?;
    let seed = std::env::var("RNGIT_IDENTITY_SEED")
        .unwrap_or_else(|_| "git-remote-rns".to_string());
    let mut client = ReticulumGitClient::default();
    client.attach_native_tcp(PrivateIdentity::new_from_name(&seed), endpoint);

    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut fetches = Vec::new();
    for line in stdin.lock().lines() {
        let line = line?;
        let mut parts = line.split_whitespace();
        match parts.next().unwrap_or_default() {
            "capabilities" => {
                stdout.write_all(b"list\nfetch\noption\n\n")?;
                stdout.flush()?;
            }
            "list" => {
                let for_push = match parts.next() {
                    None => false,
                    Some("for-push") => true,
                    Some(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "unsupported list option",
                        ));
                    }
                };
                if parts.next().is_some() {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported list option"));
                }
                let refs = client.handle_git_list(&remote, for_push).map_err(io::Error::other)?;
                for reference in refs {
                    if let Some((sha, name)) = reference.split_once(' ') {
                        if san_ref(name).is_some() && san_sha(sha).is_some() {
                            writeln!(stdout, "{sha} {name}")?;
                        }
                    }
                }
                stdout.write_all(b"\n")?;
                stdout.flush()?;
            }
            "fetch" => {
                let sha = parts.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "fetch requires an object ID and ref")
                })?;
                let reference = parts.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "fetch requires an object ID and ref")
                })?;
                if parts.next().is_some() || san_sha(sha).is_none() || san_ref(reference).is_none() {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid fetch request"));
                }
                if !fetches.iter().any(|(_, existing)| existing == reference) {
                    fetches.push((sha.to_string(), reference.to_string()));
                }
            }
            "" => {
                if !fetches.is_empty() {
                    let refs = fetches.iter().map(|(_, reference)| reference.clone()).collect::<Vec<_>>();
                    let response = client.process_fetch_queue(&remote, &refs).map_err(io::Error::other)?;
                    if response.first().copied() != Some(0) {
                        let detail = String::from_utf8_lossy(response.get(1..).unwrap_or_default());
                        return Err(io::Error::other(format!("remote fetch failed: {detail}")));
                    }
                    if response.len() > 1 {
                        let bundle = tempfile::Builder::new().prefix("rngit-helper-").suffix(".bundle").tempfile()?;
                        std::fs::write(bundle.path(), &response[1..])?;
                        let verified = Command::new("git")
                            .args(["bundle", "verify", "-q"])
                            .arg(bundle.path())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status()?;
                        if !verified.success() {
                            return Err(io::Error::new(io::ErrorKind::InvalidData, "downloaded Git bundle failed verification"));
                        }
                        let unbundled = Command::new("git")
                            .args(["bundle", "unbundle"])
                            .arg(bundle.path())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status()?;
                        if !unbundled.success() {
                            return Err(io::Error::other("Git could not import downloaded bundle"));
                        }
                        for (sha, _) in &fetches {
                            let object = Command::new("git")
                                .args(["cat-file", "-e"])
                                .arg(format!("{sha}^{{object}}"))
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .status()?;
                            if !object.success() {
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    format!("downloaded Git bundle did not provide requested object {sha}"),
                                ));
                            }
                        }
                    }
                    fetches.clear();
                }
                stdout.write_all(b"\n")?;
                stdout.flush()?;
            }
            "option" => {
                let option = parts.next().unwrap_or_default();
                let result = if option == "progress" { "ok" } else { "unsupported" };
                writeln!(stdout, "{result}")?;
                stdout.flush()?;
            }
            command => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsupported remote-helper command: {command}"),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_helper_rejects_invalid_git_references() {
        assert!(san_ref("refs/heads/main").is_some());
        assert!(san_ref("not-a-ref").is_none());
    }
}
