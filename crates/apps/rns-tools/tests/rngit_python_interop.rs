use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn free_port() -> io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn wait_for_port(port: u16, child: &mut Child) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "rngit exited before opening its port: {status}"
            )));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "rngit did not open its TCP port"))
}

fn python_repo() -> PathBuf {
    let configured = std::env::var_os("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".tmp/python-refs/Reticulum"));
    if configured.is_absolute() {
        configured
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").join(configured)
    }
}

fn python_bin() -> String {
    std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string())
}

fn write_python_config(path: &Path, port: u16) -> io::Result<()> {
    fs::write(
        path.join("config"),
        format!(
            "[reticulum]\n\
             enable_transport = no\n\
             share_instance = no\n\
             \n\
             [logging]\n\
             loglevel = 0\n\
             \n\
             [interfaces]\n\
             [[TCP Client Interface]]\n\
             type = TCPClientInterface\n\
             enabled = yes\n\
             target_host = 127.0.0.1\n\
             target_port = {port}\n"
        ),
    )
}

fn run_git(directory: &Path, args: &[&str]) -> io::Result<()> {
    let output = Command::new("git").args(args).current_dir(directory).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn create_repository_fixture(temp: &Path) -> io::Result<PathBuf> {
    let root = temp.join("rngit-root");
    let group = root.join("group");
    let repository = group.join("repo");
    let source = temp.join("source");
    fs::create_dir_all(&group)?;
    fs::create_dir_all(&source)?;

    run_git(&source, &["init", "-q"])?;
    run_git(&source, &["config", "user.email", "rngit-python@example.invalid"])?;
    run_git(&source, &["config", "user.name", "rngit-python-interop"])?;
    run_git(&source, &["checkout", "-qb", "main"])?;
    fs::write(source.join("README.md"), b"# Python rngit interop\n")?;
    let media = (0..8192).map(|index| (index as u8).wrapping_mul(29)).collect::<Vec<_>>();
    fs::write(source.join("image.png"), &media)?;
    run_git(&source, &["add", "README.md", "image.png"])?;
    run_git(&source, &["commit", "-qm", "interop fixture"])?;

    run_git(&group, &["init", "--bare", "-q", "repo"])?;
    run_git(&repository, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    let repository_url = repository.to_string_lossy().into_owned();
    run_git(&source, &["remote", "add", "origin", &repository_url])?;
    run_git(&source, &["push", "-q", "origin", "main"])?;

    // The process-facing node has no test-only permission mutation hook. The
    // group sidecar gives the Python client the same read access as the
    // local page fixtures while keeping the production loader in the path.
    fs::write(
        root.join("group.allowed"),
        "read:all\nwrite:all\ncreate:all\nstats:all\nrelease:all\n",
    )?;
    Ok(root)
}

fn rust_destination(root: &Path, identity_seed: &str) -> io::Result<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--print-identity",
            "--identity-seed",
            identity_seed,
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "rngit identity query failed: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Listening on : "))
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            io::Error::other(format!(
                "rngit identity query omitted destination:\n{}",
                String::from_utf8_lossy(&output.stdout)
            ))
        })
}

fn rust_git_destination(root: &Path, identity_seed: &str) -> io::Result<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--print-identity",
            "--identity-seed",
            identity_seed,
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "rngit Git identity query failed: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Git listening on : "))
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            io::Error::other(format!(
                "rngit identity query omitted Git destination:\n{}",
                String::from_utf8_lossy(&output.stdout)
            ))
        })
}

fn run_python_client(
    repo: &Path,
    config_dir: &Path,
    identity: &Path,
    destination: &str,
) -> io::Result<Output> {
    // This is intentionally a small NomadNet-compatible client rather than
    // a Rust-side protocol fixture: RNS.Link.request performs the pinned
    // Python msgpack/request-id/Resource handling used by real clients.
    const CLIENT: &str = r#"
import hashlib
import json
import os
import sys
import threading
import time
import RNS

config_dir, identity_path, destination_hex = sys.argv[1:4]
RNS.Reticulum(configdir=config_dir, loglevel=0)
identity = RNS.Identity.from_file(identity_path) if os.path.isfile(identity_path) else RNS.Identity()
if not os.path.isfile(identity_path):
    identity.to_file(identity_path)

destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit destination")
remote_identity = RNS.Identity.recall(destination_hash)
if remote_identity is None:
    raise RuntimeError("could not recall rngit identity")

destination = RNS.Destination(
    remote_identity,
    RNS.Destination.OUT,
    RNS.Destination.SINGLE,
    "nomadnetwork",
    "node",
)
link_ready = threading.Event()
link_failed = []

def established(link):
    link.identify(identity)
    link_ready.set()

def closed(link):
    if not link_ready.is_set():
        link_failed.append("link closed before activation")
        link_ready.set()

link = RNS.Link(destination)
link.set_link_established_callback(established)
link.set_link_closed_callback(closed)
if not link_ready.wait(30):
    raise RuntimeError("rngit link establishment timed out")
if link_failed:
    raise RuntimeError(link_failed[0])

def request(path, data):
    finished = threading.Event()
    result = {}
    def response(receipt):
        value = receipt.response
        if hasattr(value, "read"):
            payload = value.read()
            metadata = receipt.metadata or {}
            name = metadata.get("name", b"")
            result["name"] = name.decode("utf-8") if isinstance(name, bytes) else str(name)
        else:
            payload = value
            result["name"] = None
        result["sha256"] = hashlib.sha256(payload).hexdigest()
        result["size"] = len(payload)
        result["page_has_repository"] = b"Repository" in payload
        finished.set()
    def failed(receipt):
        result["error"] = "request failed"
        finished.set()
    receipt = link.request(
        path,
        data,
        response_callback=response,
        failed_callback=failed,
        timeout=30,
    )
    if receipt is False:
        raise RuntimeError("request was not sent")
    if not finished.wait(30):
        raise RuntimeError("request timed out: " + path)
    if "error" in result:
        raise RuntimeError(result["error"] + ": " + path)
    return result

page = request(
    "/page/repo.mu",
    {"var_g": "group", "var_r": "repo", "var_ref": "HEAD"},
)
media = request(
    "/media",
    {
        "key": b"rngit-python-interop",
        "path": "/media/group/repo/HEAD/image.png",
    },
)
link.teardown()
print(json.dumps({"page": page, "media": media}, sort_keys=True))
"#;
    Command::new(python_bin())
        .arg("-c")
        .arg(CLIENT)
        .arg(config_dir)
        .arg(identity)
        .arg(destination)
        .env("PYTHONPATH", repo)
        .output()
}

fn run_python_git_client(
    repo: &Path,
    config_dir: &Path,
    identity: &Path,
    destination: &str,
) -> io::Result<Output> {
    const CLIENT: &str = r#"
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import threading
import time
import RNS

config_dir, identity_path, destination_hex = sys.argv[1:4]
RNS.Reticulum(configdir=config_dir, loglevel=0)
identity = RNS.Identity.from_file(identity_path) if os.path.isfile(identity_path) else RNS.Identity()
if not os.path.isfile(identity_path):
    identity.to_file(identity_path)

destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit Git destination")
remote_identity = RNS.Identity.recall(destination_hash)
if remote_identity is None:
    raise RuntimeError("could not recall rngit Git identity")

destination = RNS.Destination(
    remote_identity,
    RNS.Destination.OUT,
    RNS.Destination.SINGLE,
    "git",
    "repositories",
)
link_ready = threading.Event()
link_failed = []

def established(link):
    link.identify(identity)
    link_ready.set()

def closed(link):
    if not link_ready.is_set():
        link_failed.append("Git link closed before activation")
        link_ready.set()

link = RNS.Link(destination)
link.set_link_established_callback(established)
link.set_link_closed_callback(closed)
if not link_ready.wait(30):
    raise RuntimeError("rngit Git link establishment timed out")
if link_failed:
    raise RuntimeError(link_failed[0])

def request(path, data):
    finished = threading.Event()
    result = {}
    def response(receipt):
        value = receipt.response
        result["payload"] = value.read() if hasattr(value, "read") else value
        finished.set()
    def failed(receipt):
        result["error"] = "request failed: " + path
        finished.set()
    receipt = link.request(
        path,
        data,
        response_callback=response,
        failed_callback=failed,
        timeout=30,
    )
    if receipt is False:
        raise RuntimeError("request was not sent: " + path)
    if not finished.wait(30):
        raise RuntimeError("request timed out: " + path)
    if "error" in result:
        raise RuntimeError(result["error"])
    payload = result["payload"]
    if not isinstance(payload, bytes):
        raise RuntimeError("response was not bytes: " + path)
    return payload

listing = request(
    "/git/list",
    {0: "group/repo", "for_push": False},
)
if listing[0] != 0 or b"refs/heads/main" not in listing:
    raise RuntimeError("Git list response was not successful")
main_sha = next(
    line.split(b" ", 1)[0]
    for line in listing[1:].splitlines()
    if line.endswith(b" refs/heads/main")
)
bundle_response = request(
    "/git/fetch",
    {
        0: "group/repo",
        "refs": [{"ref": "refs/heads/main", "sha": main_sha.decode("ascii")}],
    },
)
bundle = bundle_response[1:] if bundle_response[0] == 0 else b""
with tempfile.NamedTemporaryFile() as bundle_file:
    bundle_file.write(bundle)
    bundle_file.flush()
    verification = subprocess.run(
        ["git", "bundle", "verify", "-q", bundle_file.name],
        capture_output=True,
        check=False,
    )

result = {
    "sha256": hashlib.sha256(listing).hexdigest(),
    "status": listing[0],
    "contains_main": b"refs/heads/main" in listing,
    "fetch_status": bundle_response[0],
    "fetch_size": len(bundle),
    "fetch_sha256": hashlib.sha256(bundle).hexdigest(),
    "fetch_valid": verification.returncode == 0,
}
push_response = request(
    "/git/push",
    {
        0: "group/repo",
        "local_ref": "refs/heads/main",
        "remote_ref": "refs/heads/python",
        "bundle": bundle,
    },
)
if push_response[0] != 0:
    raise RuntimeError("Git push response was not successful")
after_push = request(
    "/git/list",
    {0: "group/repo", "for_push": False},
)
if after_push[0] != 0 or b"refs/heads/python" not in after_push:
    raise RuntimeError("Git push did not create the requested ref")
result["push_status"] = push_response[0]
result["push_contains_python_ref"] = b"refs/heads/python" in after_push
delete_response = request(
    "/git/delete",
    {0: "group/repo", "ref": "refs/heads/python"},
)
if delete_response[0] != 0:
    raise RuntimeError("Git delete response was not successful")
after_delete = request(
    "/git/list",
    {0: "group/repo", "for_push": False},
)
if after_delete[0] != 0 or b"refs/heads/python" in after_delete:
    raise RuntimeError("Git delete did not remove the requested ref")
create_response = request(
    "/git/create",
    {0: "group/newrepo"},
)
if create_response[0] != 0:
    raise RuntimeError("Git create response was not successful")
created_listing = request(
    "/git/list",
    {0: "group/newrepo", "for_push": False},
)
if created_listing[0] != 0 or b" HEAD" not in created_listing:
    raise RuntimeError("Git create did not register the new repository")
result["delete_status"] = delete_response[0]
result["delete_removed_python_ref"] = b"refs/heads/python" not in after_delete
result["create_status"] = create_response[0]
result["create_registered_repository"] = created_listing[0] == 0
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;
    Command::new(python_bin())
        .arg("-c")
        .arg(CLIENT)
        .arg(config_dir)
        .arg(identity)
        .arg(destination)
        .env("PYTHONPATH", repo)
        .output()
}

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn rngit_serves_pages_and_media_to_pinned_python_client() -> io::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let python_repo = python_repo();
    if !python_repo.join("RNS/Link.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }

    let port = free_port()?;
    let identity_seed = "rngit-python-interop-server";
    let mut server = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            identity_seed,
            "--no-media-conversion",
            "--silent",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let destination = rust_destination(&root, identity_seed)?;
        let config_dir = temp.path().join("python-client");
        fs::create_dir_all(&config_dir)?;
        write_python_config(&config_dir, port)?;
        let identity = config_dir.join("identity");
        let output = run_python_client(&python_repo, &config_dir, &identity, &destination)?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python rngit client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("\"page_has_repository\": true"), "page response: {stdout}");
        assert!(stdout.contains("\"name\": \"image.png\""), "media metadata: {stdout}");
        assert!(
            stdout.contains(
                "\"sha256\": \"f8e920545e99cdc9bbc2650eb8282344e8971a7ff0c397c91355d0fcaf6c61fa\""
            ),
            "media checksum: {stdout}"
        );
        assert!(stdout.contains("\"size\": 8192"), "media size: {stdout}");

        let git_destination = rust_git_destination(&root, identity_seed)?;
        let git_config_dir = temp.path().join("python-git-client");
        fs::create_dir_all(&git_config_dir)?;
        write_python_config(&git_config_dir, port)?;
        let git_identity = git_config_dir.join("identity");
        let git_output =
            run_python_git_client(&python_repo, &git_config_dir, &git_identity, &git_destination)?;
        if !git_output.status.success() {
            return Err(io::Error::other(format!(
                "Python rngit Git client failed: {}\nstdout:\n{}\nstderr:\n{}",
                git_output.status,
                String::from_utf8_lossy(&git_output.stdout),
                String::from_utf8_lossy(&git_output.stderr)
            )));
        }
        let git_stdout = String::from_utf8_lossy(&git_output.stdout);
        assert!(git_stdout.contains("\"status\": 0"), "Git list status: {git_stdout}");
        assert!(git_stdout.contains("\"contains_main\": true"), "Git list payload: {git_stdout}");
        assert!(git_stdout.contains("\"fetch_status\": 0"), "Git fetch status: {git_stdout}");
        assert!(git_stdout.contains("\"fetch_valid\": true"), "Git fetch bundle: {git_stdout}");
        assert!(!git_stdout.contains("\"fetch_size\": 0"), "Git fetch was empty: {git_stdout}");
        assert!(git_stdout.contains("\"push_status\": 0"), "Git push status: {git_stdout}");
        assert!(
            git_stdout.contains("\"push_contains_python_ref\": true"),
            "Git push ref listing: {git_stdout}"
        );
        assert!(git_stdout.contains("\"delete_status\": 0"), "Git delete status: {git_stdout}");
        assert!(
            git_stdout.contains("\"delete_removed_python_ref\": true"),
            "Git delete ref listing: {git_stdout}"
        );
        assert!(git_stdout.contains("\"create_status\": 0"), "Git create status: {git_stdout}");
        assert!(
            git_stdout.contains("\"create_registered_repository\": true"),
            "Git create listing: {git_stdout}"
        );
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}
