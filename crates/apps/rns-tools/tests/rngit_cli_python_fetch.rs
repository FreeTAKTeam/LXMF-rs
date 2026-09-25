use std::fs;
use std::io::{self, BufRead, BufReader};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const PINNED_RETICULUM_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";

fn run_git(directory: &Path, args: &[&str]) -> io::Result<String> {
    let output = Command::new("git").args(args).current_dir(directory).output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(io::Error::other(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn run_git_with_env(
    directory: &Path,
    args: &[&str],
    helper_dir: &Path,
    port: u16,
) -> io::Result<String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut search_path = std::env::split_paths(&path).collect::<Vec<_>>();
    search_path.insert(0, helper_dir.to_path_buf());
    let search_path = std::env::join_paths(search_path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .env("PATH", search_path)
        .env("RNGIT_CONNECT", format!("127.0.0.1:{port}"))
        .env("RNGIT_IDENTITY_SEED", "git-remote-rns-python-fetch-test")
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(io::Error::other(format!(
            "git {args:?} via Rust remote helper failed: {}\nstdout:\n{}",
            String::from_utf8_lossy(&output.stderr).trim(),
            String::from_utf8_lossy(&output.stdout)
        )))
    }
}

fn free_port() -> io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn python_repo() -> std::path::PathBuf {
    let configured = std::env::var_os("RETICULUM_PY_REPO")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(".tmp/python-refs/Reticulum"));
    if configured.is_absolute() {
        configured
    } else {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").join(configured)
    }
}

fn wait_for_port(port: u16, child: &mut Child) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!("pinned Python rngit exited early: {status}")));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "pinned Python rngit did not open its TCP port"))
}

fn write_rns_config(directory: &Path, port: u16) -> io::Result<()> {
    fs::write(
        directory.join("config"),
        format!(
            "[reticulum]\n\
             enable_transport = no\n\
             share_instance = no\n\
             \n\
             [logging]\n\
             loglevel = 0\n\
             \n\
             [interfaces]\n\
             [[TCP Server Interface]]\n\
             type = TCPServerInterface\n\
             enabled = yes\n\
             listen_ip = 127.0.0.1\n\
             listen_port = {port}\n"
        ),
    )
}

#[test]
#[ignore = "requires the pinned Python Reticulum checkout"]
fn rngit_cli_fetches_and_pushes_with_python_service() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let python_repo = python_repo();
    let actual_revision = run_git(&python_repo, &["rev-parse", "HEAD"])?;
    if actual_revision != PINNED_RETICULUM_REVISION {
        return Err(io::Error::other(format!(
            "expected pinned Reticulum {PINNED_RETICULUM_REVISION}, found {actual_revision}"
        )));
    }
    if !python_repo.join("RNS/Utilities/rngit/server.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rngit server not found under {}", python_repo.display()),
        ));
    }

    let service_root = temp.path().join("python-service");
    let group_root = service_root.join("group");
    let remote_repo = group_root.join("repo");
    let source_repo = temp.path().join("source-repository");
    let local_repo = temp.path().join("local-repository");
    let rns_config = temp.path().join("python-rns-config");
    let rngit_config = temp.path().join("python-rngit-config");
    for directory in [&group_root, &source_repo, &local_repo, &rns_config, &rngit_config] {
        fs::create_dir_all(directory)?;
    }

    run_git(&source_repo, &["init", "-q"])?;
    run_git(&source_repo, &["checkout", "-q", "-b", "main"])?;
    run_git(&source_repo, &["config", "user.email", "rngit-fetch@example.invalid"])?;
    run_git(&source_repo, &["config", "user.name", "rngit-fetch-test"])?;
    let payload = (0..32_771)
        .map(|index| (index as u8).wrapping_mul(73).wrapping_add((index >> 8) as u8))
        .collect::<Vec<_>>();
    fs::write(source_repo.join("binary-fixture.dat"), &payload)?;
    run_git(&source_repo, &["add", "binary-fixture.dat"])?;
    run_git(&source_repo, &["commit", "-q", "-m", "pinned rngit fetch fixture"])?;
    let expected_head = run_git(&source_repo, &["rev-parse", "HEAD"])?;
    run_git(&local_repo, &["init", "-q"])?;
    run_git(
        temp.path(),
        &[
            "clone",
            "-q",
            "--bare",
            source_repo.to_string_lossy().as_ref(),
            remote_repo.to_string_lossy().as_ref(),
        ],
    )?;
    fs::write(service_root.join("group.allowed"), "read:all\nwrite:all\n")?;
    fs::write(remote_repo.with_extension("allowed"), "read:all\nwrite:all\n")?;
    fs::write(
        rngit_config.join("config"),
        format!("[repositories]\ngroup = {}\n", group_root.display()),
    )?;

    let port = free_port()?;
    write_rns_config(&rns_config, port)?;
    let server_script = r#"
import sys
import time
import RNS
from RNS.Utilities.rngit.server import ReticulumGitNode

RNS.Reticulum(configdir=sys.argv[2], loglevel=0)
node = ReticulumGitNode(configdir=sys.argv[1], verbosity=0)
if not node.ready:
    raise RuntimeError("Python rngit node was not ready")
node.start()
print(node.destination.hash.hex(), flush=True)
while node._should_run:
    node.announce()
    time.sleep(1)
"#;
    let mut server =
        Command::new(std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".into()))
            .arg("-c")
            .arg(server_script)
            .arg(&rngit_config)
            .arg(&rns_config)
            .env("PYTHONPATH", &python_repo)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let stdout = server
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Python rngit stdout was not captured"))?;
        let (destination_tx, destination_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut destination = String::new();
            let result = BufReader::new(stdout).read_line(&mut destination).map(|_| destination);
            let _ = destination_tx.send(result);
        });
        let destination =
            destination_rx.recv_timeout(Duration::from_secs(8)).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("Python rngit did not report its destination: {error}"),
                )
            })??;
        let destination = destination.trim();
        if destination.len() != 32 || !destination.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::other(format!(
                "Python rngit returned invalid destination {destination:?}"
            )));
        }

        let client = Command::new(env!("CARGO_BIN_EXE_rngit"))
            .args([
                "--root",
                local_repo.to_string_lossy().as_ref(),
                "--connect",
                &format!("127.0.0.1:{port}"),
                "--identity-seed",
                "rngit-cli-python-fetch-client",
                "fetch",
                &format!("rns://{destination}/group/repo"),
                "refs/heads/main",
                "refs/remotes/rns/main",
            ])
            .output()?;
        if !client.status.success() {
            return Err(io::Error::other(format!(
                "production rngit fetch CLI failed: {}\nstdout:\n{}\nstderr:\n{}",
                client.status,
                String::from_utf8_lossy(&client.stdout),
                String::from_utf8_lossy(&client.stderr)
            )));
        }
        assert!(
            String::from_utf8_lossy(&client.stdout).contains("Fetched refs/heads/main"),
            "CLI did not report a completed fetch: {}",
            String::from_utf8_lossy(&client.stdout)
        );
        assert_eq!(run_git(&local_repo, &["rev-parse", "refs/remotes/rns/main"])?, expected_head);
        let fetched_blob = Command::new("git")
            .arg("-C")
            .arg(&local_repo)
            .args(["cat-file", "blob", "refs/remotes/rns/main:binary-fixture.dat"])
            .output()?;
        if !fetched_blob.status.success() {
            return Err(io::Error::other(format!(
                "could not read fetched binary blob: {}",
                String::from_utf8_lossy(&fetched_blob.stderr)
            )));
        }
        assert_eq!(fetched_blob.stdout, payload);

        let pushed_payload = (0..19_337)
            .map(|index| (index as u8).wrapping_mul(29).wrapping_add((index >> 7) as u8))
            .collect::<Vec<_>>();
        fs::write(source_repo.join("pushed-fixture.dat"), &pushed_payload)?;
        run_git(&source_repo, &["add", "pushed-fixture.dat"])?;
        run_git(&source_repo, &["commit", "-q", "-m", "pinned rngit push fixture"])?;
        let pushed_head = run_git(&source_repo, &["rev-parse", "HEAD"])?;
        let client = Command::new(env!("CARGO_BIN_EXE_rngit"))
            .args([
                "--root",
                source_repo.to_string_lossy().as_ref(),
                "--connect",
                &format!("127.0.0.1:{port}"),
                "--identity-seed",
                "rngit-cli-python-push-client",
                "push",
                &format!("rns://{destination}/group/repo"),
                "refs/heads/main",
                "refs/heads/rust-pushed",
            ])
            .output()?;
        if !client.status.success() {
            return Err(io::Error::other(format!(
                "production rngit push CLI failed: {}\nstdout:\n{}\nstderr:\n{}",
                client.status,
                String::from_utf8_lossy(&client.stdout),
                String::from_utf8_lossy(&client.stderr)
            )));
        }
        assert_eq!(run_git(&remote_repo, &["rev-parse", "refs/heads/rust-pushed"])?, pushed_head);
        let pushed_blob = Command::new("git")
            .arg("--git-dir")
            .arg(&remote_repo)
            .args(["cat-file", "blob", "refs/heads/rust-pushed:pushed-fixture.dat"])
            .output()?;
        if !pushed_blob.status.success() {
            return Err(io::Error::other(format!(
                "could not read pushed binary blob: {}",
                String::from_utf8_lossy(&pushed_blob.stderr)
            )));
        }
        assert_eq!(pushed_blob.stdout, pushed_payload);

        let helper_repo = temp.path().join("remote-helper-repository");
        let helper_dir = temp.path().join("remote-helpers");
        fs::create_dir_all(&helper_repo)?;
        fs::create_dir_all(&helper_dir)?;
        run_git(&helper_repo, &["init", "-q"])?;
        fs::copy(env!("CARGO_BIN_EXE_git-remote-rns"), helper_dir.join("git-remote-rns"))?;
        let remote_url = format!("rns://{destination}/group/repo");

        let listed =
            run_git_with_env(&helper_repo, &["ls-remote", &remote_url], &helper_dir, port)?;
        assert!(
            listed.lines().any(|line| line == format!("{expected_head}\trefs/heads/main")),
            "Git remote-helper discovery did not advertise the exact Python ref: {listed}"
        );

        let fetched = run_git_with_env(
            &helper_repo,
            &["fetch", &remote_url, "refs/heads/main:refs/remotes/rns/main"],
            &helper_dir,
            port,
        )?;
        assert_eq!(run_git(&helper_repo, &["rev-parse", "refs/remotes/rns/main"])?, expected_head);
        let helper_blob = Command::new("git")
            .arg("cat-file")
            .arg("blob")
            .arg("refs/remotes/rns/main:binary-fixture.dat")
            .current_dir(&helper_repo)
            .output()?;
        if !helper_blob.status.success() {
            return Err(io::Error::other(format!(
                "could not read remote-helper fetched blob: {}\nfetch output: {fetched}",
                String::from_utf8_lossy(&helper_blob.stderr)
            )));
        }
        assert_eq!(helper_blob.stdout, payload);
        Ok(())
    })();

    let _ = server.kill();
    let _ = server.wait()?;
    result
}
