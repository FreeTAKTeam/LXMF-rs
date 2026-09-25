use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[path = "rngit_python_interop/issue_613_cleanup.rs"]
mod issue_613_cleanup;
#[cfg(unix)]
#[path = "rngit_python_interop/issue_613_cleanup_isolation.rs"]
mod issue_613_cleanup_isolation;
#[path = "rngit_python_interop/issue_613_configured_backend.rs"]
mod issue_613_configured_backend;
#[path = "rngit_python_interop/issue_613_media_access.rs"]
mod issue_613_media_access;
#[path = "rngit_python_interop/issue_613_media_compression.rs"]
mod issue_613_media_compression;
#[path = "rngit_python_interop/issue_613_media_invalid_ref.rs"]
mod issue_613_media_invalid_ref;
#[path = "rngit_python_interop/issue_613_media_options.rs"]
mod issue_613_media_options;
#[cfg(unix)]
#[path = "rngit_python_interop/issue_613_media_read_failure.rs"]
mod issue_613_media_read_failure;
#[cfg(unix)]
#[path = "rngit_python_interop/issue_613_media_source_open_failure.rs"]
mod issue_613_media_source_open_failure;
#[path = "rngit_python_interop/issue_613_media_traversal.rs"]
mod issue_613_media_traversal;
#[path = "rngit_python_interop/issue_613_media_url.rs"]
mod issue_613_media_url;
#[cfg(unix)]
#[path = "rngit_python_interop/issue_613_media_zero_stat.rs"]
mod issue_613_media_zero_stat;
#[path = "rngit_python_interop/issue_613_no_ident.rs"]
mod issue_613_no_ident;
#[cfg(target_os = "linux")]
#[path = "rngit_python_interop/issue_613_output_open_failure.rs"]
mod issue_613_output_open_failure;
#[cfg(unix)]
#[path = "rngit_python_interop/issue_613_output_write_failure.rs"]
mod issue_613_output_write_failure;
#[cfg(unix)]
#[path = "rngit_python_interop/issue_613_temp_directory_failure.rs"]
mod issue_613_temp_directory_failure;
#[cfg(unix)]
#[path = "rngit_python_interop/issue_613_template_launch_failure.rs"]
mod issue_613_template_launch_failure;

static PYTHON_INTEROP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[path = "rngit_python_interop/typed_msgpack.rs"]
mod typed_msgpack;
#[path = "rngit_python_interop/work_request_cases.rs"]
mod work_request_cases;

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

fn wait_for_empty_directory(directory: &Path) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if fs::read_dir(directory)?.next().transpose()?.is_none() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("temporary media files remain under {}", directory.display()),
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
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
    fs::create_dir_all(source.join("assets"))?;
    fs::write(source.join("assets/space name.bin"), b"percent decoded media path\0\xff\n")?;
    fs::write(source.join("assets/nested image.png"), b"\x89PNG\xffnested image")?;
    fs::write(source.join("large.png"), vec![0x5a; 8 * 1024 * 1024])?;
    fs::write(
        source.join("valid.png"),
        [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00,
            0x00, 0xb5, 0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xda, 0x63, 0x64, 0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66,
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ],
    )?;
    run_git(&source, &["add", "README.md", "image.png", "large.png", "valid.png", "assets"])?;
    run_git(&source, &["commit", "-qm", "interop fixture"])?;

    run_git(&group, &["init", "--bare", "-q", "repo"])?;
    run_git(&repository, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    let repository_url = repository.to_string_lossy().into_owned();
    let source_url = source.to_string_lossy().into_owned();
    run_git(&source, &["remote", "add", "origin", &repository_url])?;
    run_git(&source, &["push", "-q", "origin", "main"])?;
    run_git(&repository, &["remote", "add", "upstream", &source_url])?;

    // The process-facing node has no test-only permission mutation hook. The
    // group sidecar gives the Python client the same read access as the
    // local page fixtures while keeping the production loader in the path.
    fs::write(
        root.join("group.allowed"),
        "read:all\nwrite:all\ncreate:all\nstats:all\nrelease:all\ninteract:all\nadmin:all\n",
    )?;
    let private_group = root.join("private");
    let private_repository = private_group.join("repo");
    fs::create_dir_all(&private_group)?;
    run_git(&private_group, &["init", "--bare", "-q", "repo"])?;
    run_git(&private_repository, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    fs::write(root.join("private.allowed"), "read:all\n")?;
    fs::write(private_repository.with_extension("allowed"), "read:none\n")?;
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
    media_temp_directory: &Path,
) -> io::Result<Output> {
    const CLIENT: &str = r#"
import hashlib
import json
import os
import sys
import threading
import time
import urllib.parse
import RNS

config_dir, identity_path, destination_hex, media_temp_directory = sys.argv[1:5]
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
        if not hasattr(value, "read"):
            result["body"] = payload.decode("utf-8", errors="replace")
        result["page_has_repository"] = b"Repository" in payload
        result["has_not_found"] = b"Not Found" in payload
        result["has_ref_not_found"] = b"reference was not found" in payload
        result["has_file_not_found"] = b"file was not found" in payload
        result["is_readme"] = payload == b'# Python rngit interop\n'
        result["is_webp"] = len(payload) >= 12 and payload[:4] == b"RIFF" and payload[8:12] == b"WEBP"
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

def request_media_denial(path, data):
    finished = threading.Event()
    result = {
        "failed": False,
        "timed_out": False,
        "response_received": False,
        "response_is_false": False,
        "metadata_present": False,
        "media_bytes_received": False,
    }
    def response(receipt):
        value = receipt.response
        result["response_received"] = True
        result["response_is_false"] = value is False
        result["metadata_present"] = receipt.metadata is not None
        if hasattr(value, "read"):
            result["media_bytes_received"] = bool(value.read())
        elif isinstance(value, (bytes, bytearray)):
            result["media_bytes_received"] = bool(value)
        finished.set()
    def failed(receipt):
        result["failed"] = True
        finished.set()
    receipt = link.request(
        path,
        data,
        response_callback=response,
        failed_callback=failed,
        timeout=3,
    )
    if receipt is False:
        result["failed"] = True
        return result
    if not finished.wait(5):
        result["timed_out"] = True
    return result

page = request(
    "/page/repo.mu",
    {"var_g": "group", "var_r": "repo", "var_ref": "HEAD"},
)
missing_repository = request(
    "/page/repo.mu",
    {"var_g": "missing", "var_r": "repo", "var_ref": "HEAD"},
)
invalid_reference = request(
    "/page/repo.mu",
    {"var_g": "group", "var_r": "repo", "var_ref": "refs/heads/missing"},
)
missing_blob = request(
    "/page/blob.mu",
    {
        "var_g": "group",
        "var_r": "repo",
        "var_ref": "HEAD",
        "var_path": "missing.txt",
    },
)
denied_repository = request(
    "/page/repo.mu",
    {"var_g": "private", "var_r": "repo", "var_ref": "HEAD"},
)
download = request(
    "/file/download",
    {
        "var_g": "group",
        "var_r": "repo",
        "var_ref": "HEAD",
        "var_path": "README.md",
    },
)
media = request(
    "/media",
    {
        "key": b"rngit-python-interop",
        "path": "/media/group/repo/HEAD/image.png",
    },
)
image_page = request(
    "/page/blob.mu",
    {
        "var_g": "group",
        "var_r": "repo",
        "var_ref": "HEAD",
        "var_path": "assets/nested image.png",
    },
)
expected_image_markup = "`(Image file`w=n`a=c`:/media/group/repo/HEAD/" + urllib.parse.quote_plus("assets/nested image.png") + ")"
if expected_image_markup not in image_page["body"]:
    raise RuntimeError(f"nested image markup differs from pinned quote_plus behavior: {image_page['body']}")
converted_media = request(
    "/media",
    {
        "key": b"rngit-python-interop",
        "path": "/media/group/repo/HEAD/valid.png",
    },
)
media_temp_directories_during_link = [
    name for name in os.listdir(media_temp_directory) if name.startswith("rngit-media-")
]
if not media_temp_directories_during_link:
    raise RuntimeError("successful WebP conversion did not retain link-scoped temporary data")
missing_media_key = request_media_denial(
    "/media",
    {"path": "/media/group/repo/HEAD/image.png"},
)
missing_media_path = request_media_denial(
    "/media",
    {"key": b"rngit-python-interop"},
)
malformed_media_path = request_media_denial(
    "/media",
    {"key": b"rngit-python-interop", "path": "/media/group/repo"},
)
link.teardown()
if not missing_repository["has_not_found"]:
    raise RuntimeError("missing repository did not render a not-found page")
if not invalid_reference["has_ref_not_found"]:
    raise RuntimeError("invalid reference did not render a reference error")
if not missing_blob["has_file_not_found"]:
    raise RuntimeError("missing blob did not render a file error")
if not denied_repository["has_not_found"]:
    raise RuntimeError("denied repository exposed a page")
if download["name"] != "README.md" or not download["is_readme"]:
    raise RuntimeError("file download did not preserve content or filename metadata")
if converted_media["name"] != "valid.webp" or not converted_media["is_webp"]:
    raise RuntimeError("media conversion did not return validated WebP metadata/content")
for label, result in [
    ("missing media key", missing_media_key),
    ("missing media path", missing_media_path),
    ("malformed media path", malformed_media_path),
]:
    if (
        result["failed"]
        or result["timed_out"]
        or not result["response_received"]
        or not result["response_is_false"]
        or result["metadata_present"]
        or result["media_bytes_received"]
    ):
        raise RuntimeError(f"{label} did not return the reference False denial: {result}")
print(json.dumps({
    "page": page,
    "missing_repository": missing_repository,
    "invalid_reference": invalid_reference,
    "missing_blob": missing_blob,
    "denied_repository": denied_repository,
    "download": download,
    "media": media,
    "image_page": image_page,
    "converted_media": converted_media,
    "media_temp_directories_during_link": len(media_temp_directories_during_link),
    "missing_media_key": missing_media_key,
    "missing_media_path": missing_media_path,
    "malformed_media_path": malformed_media_path,
}, sort_keys=True))
"#;
    Command::new(python_bin())
        .arg("-c")
        .arg(CLIENT)
        .arg(config_dir)
        .arg(identity)
        .arg(destination)
        .arg(media_temp_directory)
        .env("PYTHONPATH", repo)
        .output()
}

fn run_python_git_client(
    repo: &Path,
    config_dir: &Path,
    identity: &Path,
    destination: &str,
    source: &Path,
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
from RNS.vendor import umsgpack as mp
import RNS

config_dir, identity_path, destination_hex, source_path = sys.argv[1:5]
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
sync_response = request(
    "/git/sync",
    {0: "group/repo"},
)
if sync_response[0] != 0:
    raise RuntimeError("Git sync response was not successful")
after_sync = request(
    "/git/list",
    {0: "group/repo", "for_push": False},
)
if after_sync[0] != 0 or b"refs/remotes/upstream/main" not in after_sync:
    raise RuntimeError("Git sync did not update the configured remote")
fork_response = request(
    "/git/fork",
    {0: "group/fork", "source": source_path},
)
if fork_response[0] != 0:
    raise RuntimeError("Git fork response was not successful")
fork_listing = request(
    "/git/list",
    {0: "group/fork", "for_push": False},
)
if fork_listing[0] != 0 or b"refs/heads/main" not in fork_listing:
    raise RuntimeError("Git fork did not register the copied repository")
mirror_response = request(
    "/git/mirror",
    {0: "group/mirror", "source": source_path},
)
if mirror_response[0] != 0:
    raise RuntimeError("Git mirror response was not successful")
mirror_listing = request(
    "/git/list",
    {0: "group/mirror", "for_push": False},
)
if mirror_listing[0] != 0 or b"refs/heads/main" not in mirror_listing:
    raise RuntimeError("Git mirror did not register the copied repository")
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
result["sync_status"] = sync_response[0]
result["sync_contains_upstream_ref"] = b"refs/remotes/upstream/main" in after_sync
result["fork_status"] = fork_response[0]
result["fork_contains_main"] = b"refs/heads/main" in fork_listing
result["mirror_status"] = mirror_response[0]
result["mirror_contains_main"] = b"refs/heads/main" in mirror_listing
result["create_status"] = create_response[0]
result["create_registered_repository"] = created_listing[0] == 0

group_permissions = request(
    "/mgmt/perms",
    {2: "group", "operation": "gperms", "step": "get"},
)
if group_permissions[0] != 0:
    raise RuntimeError("group permissions response was not successful")
group_permissions_payload = mp.unpackb(group_permissions[1:])
if "read:all" not in group_permissions_payload.get("content", ""):
    raise RuntimeError("group permissions did not round-trip")
repository_permissions = request(
    "/mgmt/perms",
    {0: "group/repo", "operation": "rperms", "step": "get"},
)
if repository_permissions[0] != 0:
    raise RuntimeError("repository permissions response was not successful")

# The same established Link must observe permission changes on its next request.
revoked_permissions = request(
    "/mgmt/perms",
    {
        0: "group/repo",
        "operation": "rperms",
        "step": "set",
        "content": "read:none\nadmin:all\n",
    },
)
if revoked_permissions[0] != 0:
    raise RuntimeError("repository read revocation failed")
revoked_listing = request("/git/list", {0: "group/repo", "for_push": False})
if revoked_listing[0] != 3 or b"Not found" not in revoked_listing:
    raise RuntimeError("same-Link repository read was not denied immediately after revocation")
restored_permissions = request(
    "/mgmt/perms",
    {
        0: "group/repo",
        "operation": "rperms",
        "step": "set",
        "content": "read:all\nwrite:all\ncreate:all\nstats:all\nrelease:all\ninteract:all\nadmin:all\n",
    },
)
if restored_permissions[0] != 0:
    raise RuntimeError("repository permissions restore failed")
restored_listing = request("/git/list", {0: "group/repo", "for_push": False})
if restored_listing[0] != 0:
    raise RuntimeError("same-Link repository read did not recover after permissions restore")

work_content = "Python work document body"
invalid_work = request(
    "/mgmt/work",
    {
        0: "group/repo",
        "operation": "create",
        "title": "Invalid work",
        "content": work_content,
        "format": "markdown",
        "signature": bytes(64),
    },
)
if invalid_work[0] != 2:
    raise RuntimeError("invalid work signature was not rejected")
work_signature = identity.sign(work_content.encode("utf-8"))
work_create = request(
    "/mgmt/work",
    {
        0: "group/repo",
        "operation": "create",
        "title": "Python work",
        "content": work_content,
        "format": "markdown",
        "signature": work_signature,
    },
)
if work_create[0] != 0:
    raise RuntimeError("work create response was not successful")
work_created = mp.unpackb(work_create[1:])
work_id = work_created["id"]
if work_created.get("scope") != "active":
    raise RuntimeError("work create did not return the active scope")

__RNGIT_WORK_REQUEST_CASES__

work_list = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "list", "scope": "active"},
)
if work_list[0] != 0:
    raise RuntimeError("work list response was not successful")
work_list_payload = mp.unpackb(work_list[1:])
if not any(document.get("id") == work_id for document in work_list_payload.get("active", [])):
    raise RuntimeError("work list did not include the created document")

work_view = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "view", "doc_id": work_id, "scope": "active"},
)
if work_view[0] != 0:
    raise RuntimeError("work view response was not successful")
work_view_payload = mp.unpackb(work_view[1:])
work_meta = work_view_payload["meta"]
if work_view_payload.get("content") != work_content:
    raise RuntimeError("work view content did not round-trip")
if work_meta.get("signature") != work_signature:
    raise RuntimeError("work signature did not round-trip")
if work_meta.get("identity") != identity.get_public_key():
    raise RuntimeError("work public identity did not round-trip")

work_comment = request(
    "/mgmt/work",
    {
        0: "group/repo",
        "operation": "comment",
        "doc_id": work_id,
        "scope": "active",
        "content": "Python comment",
        "format": "markdown",
    },
)
if work_comment[0] != 0:
    raise RuntimeError("work comment response was not successful")
comment_id = mp.unpackb(work_comment[1:])["id"]

edited_content = "Python edited work document body"
edited_signature = identity.sign(edited_content.encode("utf-8"))
work_edit = request(
    "/mgmt/work",
    {
        0: "group/repo",
        "operation": "edit",
        "doc_id": work_id,
        "scope": "active",
        "title": "Edited Python work",
        "content": edited_content,
        "signature": edited_signature,
    },
)
if work_edit[0] != 0:
    raise RuntimeError("work edit response was not successful")

work_permissions = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "perms", "doc_id": work_id, "step": "get"},
)
if work_permissions[0] != 0:
    raise RuntimeError("work permissions get response was not successful")
work_permission_content = "read:all\nwrite:all\ninteract:all\nadmin:all\n"
work_permissions_set = request(
    "/mgmt/work",
    {
        0: "group/repo",
        "operation": "perms",
        "doc_id": work_id,
        "step": "set",
        "content": work_permission_content,
    },
)
if work_permissions_set[0] != 0:
    raise RuntimeError("work permissions set response was not successful")
work_permissions_after = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "perms", "doc_id": work_id, "step": "get"},
)
if mp.unpackb(work_permissions_after[1:]).get("content") != work_permission_content:
    raise RuntimeError("work permissions did not round-trip")

work_complete = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "complete", "doc_id": work_id},
)
if work_complete[0] != 0 or mp.unpackb(work_complete[1:]).get("scope") != "completed":
    raise RuntimeError("work complete response was not successful")
work_completed_view = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "view", "doc_id": work_id, "scope": "completed"},
)
if work_completed_view[0] != 0 or mp.unpackb(work_completed_view[1:])["content"] != edited_content:
    raise RuntimeError("completed work did not preserve the edited content")
work_activate = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "activate", "doc_id": work_id},
)
if work_activate[0] != 0 or mp.unpackb(work_activate[1:]).get("scope") != "active":
    raise RuntimeError("work activate response was not successful")
work_delete = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "delete", "doc_id": work_id, "scope": "active"},
)
if work_delete[0] != 0:
    raise RuntimeError("work delete response was not successful")
work_after_delete = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "list", "scope": "active"},
)
if work_after_delete[0] != 0 or mp.unpackb(work_after_delete[1:]).get("active"):
    raise RuntimeError("work delete did not remove the document")

result["group_permissions_status"] = group_permissions[0]
result["repository_permissions_status"] = repository_permissions[0]
result["invalid_work_signature_status"] = invalid_work[0]
result["work_create_status"] = work_create[0]
result["work_malformed_list_status"] = malformed_list[0]
result["work_missing_view_status"] = missing_view_id[0]
result["work_malformed_view_status"] = malformed_view_id[0]
result["work_missing_document_status"] = missing_document[0]
result["work_denied_read_status"] = denied_read[0]
result["work_list_status"] = work_list[0]
result["work_view_status"] = work_view[0]
result["work_comment_status"] = work_comment[0]
result["work_comment_id"] = comment_id
result["work_edit_status"] = work_edit[0]
result["work_permissions_status"] = work_permissions_after[0]
result["work_complete_status"] = work_complete[0]
result["work_activate_status"] = work_activate[0]
result["work_delete_status"] = work_delete[0]
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;
    let client = CLIENT.replace("__RNGIT_WORK_REQUEST_CASES__", work_request_cases::PYTHON);
    Command::new(python_bin())
        .arg("-c")
        .arg(client)
        .arg(config_dir)
        .arg(identity)
        .arg(destination)
        .arg(source)
        .env("PYTHONPATH", repo)
        .output()
}

fn run_python_work_client(
    repo: &Path,
    config_dir: &Path,
    identity: &Path,
    destination: &str,
    mode: &str,
    work_id: Option<u64>,
) -> io::Result<Output> {
    const CLIENT: &str = r#"
import json
import os
import sys
import threading
import RNS
from RNS.vendor import umsgpack as mp

config_dir, identity_path, destination_hex, mode = sys.argv[1:5]
work_id = int(sys.argv[5]) if len(sys.argv) > 5 else None
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

def request(data):
    finished = threading.Event()
    result = {}
    def response(receipt):
        result["payload"] = receipt.response
        finished.set()
    def failed(receipt):
        result["error"] = "request failed"
        finished.set()
    receipt = link.request(
        "/mgmt/work",
        data,
        response_callback=response,
        failed_callback=failed,
        timeout=30,
    )
    if receipt is False:
        raise RuntimeError("work request was not sent")
    if not finished.wait(30):
        raise RuntimeError("work request timed out")
    if "error" in result:
        raise RuntimeError(result["error"])
    return result["payload"]

if mode == "create":
    content = "Python restart work document body"
    signature = identity.sign(content.encode("utf-8"))
    created = request({
        0: "group/repo",
        "operation": "create",
        "title": "Python restart work",
        "content": content,
        "format": "markdown",
        "signature": signature,
    })
    if created[0] != 0:
        raise RuntimeError("work create response was not successful")
    payload = mp.unpackb(created[1:])
    comment = request({0: "group/repo", "operation": "comment", "doc_id": payload["id"], "scope": "active", "content": "Python restart persisted comment", "format": "markdown"})
    result = {
        "id": payload["id"],
        "scope": payload["scope"],
        "author_hex": identity.hash.hex(),
        "identity_hex": identity.get_public_key().hex(),
        "signature_hex": signature.hex(),
    }
elif mode == "verify_typed":
    viewed = request({0: "group/repo", "operation": "view", "doc_id": work_id, "scope": "active"})
    if viewed[0] != 0:
        raise RuntimeError("Rust-seeded typed work view failed")
    document = mp.unpackb(viewed[1:])
    meta = document["meta"]
    if type(document["id"]) is not int or document["id"] != work_id:
        raise RuntimeError("work ID did not remain an integer")
    if type(meta["created"]) is not int or meta["created"] != 1_700_000_123:
        raise RuntimeError("created timestamp did not remain the expected integer")
    if type(meta["edited"]) is not int or meta["edited"] != 1_700_000_456:
        raise RuntimeError("edited timestamp did not remain the expected integer")
    if meta["author"] != "000102030405060708090a0b0c0d0e0f":
        raise RuntimeError("author identity hash did not remain the expected value")
    if type(meta["identity"]) is not bytes or meta["identity"] != bytes(range(64)):
        raise RuntimeError("identity did not remain the expected MessagePack binary")
    if type(meta["signature"]) is not bytes or meta["signature"] != bytes(reversed(range(64))):
        raise RuntimeError("signature did not remain the expected MessagePack binary")
    result = {"typed_values": True}
else:
    if work_id is None:
        raise RuntimeError("verification requires a work ID")
    listed = request({0: "group/repo", "operation": "list", "scope": "active"})
    if listed[0] != 0:
        raise RuntimeError("work list response was not successful")
    listing = mp.unpackb(listed[1:])
    active = listing.get("active", [])
    persisted = any(document.get("id") == work_id for document in active)
    viewed = request({
        0: "group/repo",
        "operation": "view",
        "doc_id": work_id,
        "scope": "active",
    })
    if viewed[0] != 0:
        raise RuntimeError("work view response was not successful")
    document = mp.unpackb(viewed[1:])
    comments = document.get("comments", [])
    result = {
        "id": work_id,
        "persisted": persisted,
        "content": document.get("content"),
        "title": document.get("meta", {}).get("title"),
        "comment_persisted": len(comments) == 1 and comments[0].get("id") == 1 and comments[0].get("content") == "Python restart persisted comment",
    }
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;
    let mut command = Command::new(python_bin());
    command.arg("-c").arg(CLIENT).arg(config_dir).arg(identity).arg(destination).arg(mode);
    if let Some(work_id) = work_id {
        command.arg(work_id.to_string());
    }
    command.env("PYTHONPATH", repo).output()
}

fn spawn_rngit_server(root: &Path, port: u16, identity_seed: &str) -> io::Result<Child> {
    Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            identity_seed,
            "--silent",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

include!("rngit_python_interop/issue_612_work_restart.rs");
include!("rngit_python_interop/issue_613_page_media.rs");
