use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config,
};
use std::fs;
use std::io;
use std::process::{Command, Stdio};

const PYTHON_RETICULUM_FEATURE_REF: &str = "7f2b3b9b524c9386316379af1313b43a5e4f7a5d";

#[test]
#[ignore = "requires exact Python Reticulum 1.5.5 checkout and loopback Link"]
fn python_1_5_5_link_receives_converted_markdown_download() -> io::Result<()> {
    let _guard = super::PYTHON_INTEROP_TEST_LOCK.lock().expect("interop test mutex poisoned");
    let python_repo = python_repo();
    let revision =
        Command::new("git").args(["rev-parse", "HEAD"]).current_dir(&python_repo).output()?;
    if !revision.status.success()
        || String::from_utf8_lossy(&revision.stdout).trim() != PYTHON_RETICULUM_FEATURE_REF
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("test requires exact Python Reticulum {PYTHON_RETICULUM_FEATURE_REF}"),
        ));
    }

    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let port = free_port()?;
    let seed = "rngit-python-1-5-5-markdown";
    let mut server = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            seed,
            "--silent",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let destination = rust_destination(&root, seed)?;
        let config_dir = temp.path().join("python-client");
        fs::create_dir_all(&config_dir)?;
        write_python_config(&config_dir, port)?;
        let output = Command::new(python_bin())
            .arg("-c")
            .arg(PYTHON_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python 1.5.5 converted-download Link failed: {}\nstdout: {}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let result: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| io::Error::other(format!("invalid Python JSON: {error}")))?;
        assert_eq!(result["received"], true);
        assert_eq!(result["failed"], false);
        assert_eq!(result["matches_python_converter"], true);
        assert_eq!(result["name"], "README.mu");
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

const PYTHON_CLIENT: &str = r##"
import json
import sys
import threading
import RNS
from RNS.Utilities.rngit.util import MarkdownToMicron

config_dir, destination_hex = sys.argv[1:3]
RNS.Reticulum(configdir=config_dir, loglevel=0)
destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit destination")
identity = RNS.Identity.recall(destination_hash)
if identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "nomadnetwork", "node")
ready = threading.Event()
link = RNS.Link(destination)
link.set_link_established_callback(lambda link: ready.set())
if not ready.wait(30):
    raise RuntimeError("converted-download Link did not establish")
finished = threading.Event()
result = {"received": False, "failed": False}
def response(receipt):
    value = receipt.response
    payload = value.read() if hasattr(value, "read") else value
    expected = MarkdownToMicron(url_scope=":/page/blob.mu`g=group|r=repo|ref=HEAD|path=").format_block("# Python rngit interop\n").rstrip().encode("utf-8")
    result["received"] = payload is not False
    result["matches_python_converter"] = payload == expected
    metadata = receipt.metadata or {}
    name = metadata.get("name")
    result["name"] = name.decode("utf-8") if isinstance(name, bytes) else name
    finished.set()
def failed(receipt):
    result["failed"] = True
    finished.set()
receipt = link.request("/file/download", {"var_g": "group", "var_r": "repo", "var_ref": "HEAD", "var_path": "README.md", "var_fmt": "mu"}, response_callback=response, failed_callback=failed, timeout=8)
if receipt is False:
    result["failed"] = True
else:
    finished.wait(12)
link.teardown()
print(json.dumps(result, sort_keys=True))
"##;
