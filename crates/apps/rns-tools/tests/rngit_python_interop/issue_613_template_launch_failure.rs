use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config,
};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires local pinned Python Reticulum checkout"]
fn rngit_page_request_uses_builtin_template_when_executable_override_cannot_launch(
) -> io::Result<()> {
    let _test_guard =
        super::PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let python_repo = python_repo();
    let reference_revision = Command::new("git")
        .args(["-C", python_repo.to_string_lossy().as_ref(), "rev-parse", "HEAD"])
        .output()?;
    if !reference_revision.status.success()
        || String::from_utf8_lossy(&reference_revision.stdout).trim()
            != "99de23c040d507e3fefca19e87b182302902725d"
    {
        return Err(io::Error::other(
            "Python interop checkout is not the frozen Reticulum revision",
        ));
    }
    let reference_pages = fs::read_to_string(python_repo.join("RNS/Utilities/rngit/pages.py"))?;
    assert!(
        reference_pages.contains("except Exception as e:")
            && reference_pages.contains("Could not get dynamic template content from {path}: {e}")
            && reference_pages.contains("return None"),
        "frozen pages.py must treat executable template launch errors as unavailable"
    );

    let templates = root.join("templates");
    fs::create_dir_all(&templates)?;
    let template = templates.join("base.mu");
    fs::write(&template, "#!/definitely/missing/rngit-template-interpreter\n")?;
    let mut permissions = fs::metadata(&template)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&template, permissions)?;

    let port = free_port()?;
    let identity_seed = "rngit-python-template-launch-failure";
    let mut server = Command::new(env!("CARGO_BIN_EXE_rngit"))
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
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let destination = rust_destination(&root, identity_seed)?;
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
                "pinned Python page client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let response: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(response["default_footer"], true, "response: {response}");
        assert_eq!(response["failed_override"], false, "response: {response}");
        assert!(server.try_wait()?.is_none(), "template launch failure terminated rngit");
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

const PYTHON_CLIENT: &str = r#"
import json
import sys
import threading
import RNS

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
link.set_link_established_callback(lambda _: ready.set())
if not ready.wait(30) or link.status != RNS.Link.ACTIVE:
    raise RuntimeError("template-failure Link did not establish")
finished = threading.Event()
result = {}
def response(receipt):
    value = receipt.response
    payload = value.read() if hasattr(value, "read") else value
    result["default_footer"] = b"Served by rngit" in payload
    result["failed_override"] = b"template launch failure" in payload
    finished.set()
def failed(receipt):
    result["request_failed"] = str(receipt.status)
    finished.set()
link.request("/page/index.mu", {}, response_callback=response, failed_callback=failed, timeout=10)
if not finished.wait(15):
    raise RuntimeError("template-failure page request timed out")
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;
