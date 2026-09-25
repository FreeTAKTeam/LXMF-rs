use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CLIENT_COUNT: usize = 4;
static PYTHON_RNGIT_INTEROP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn python_repo() -> PathBuf {
    std::env::var_os("RETICULUM_PY_REPO").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.tmp/python-refs/Reticulum")
    })
}

fn python_bin() -> String {
    std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string())
}

fn free_port() -> io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn run_git(directory: &Path, args: &[&str]) -> io::Result<()> {
    let output = Command::new("git").args(args).current_dir(directory).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn create_repository_root(temp: &Path) -> io::Result<PathBuf> {
    let root = temp.join("rngit-root");
    let group = root.join("group");
    let repository = group.join("repo");
    fs::create_dir_all(&group)?;
    run_git(&group, &["init", "--bare", "-q", "repo"])?;
    run_git(&repository, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    fs::write(
        root.join("group.allowed"),
        "read:all\nwrite:all\ncreate:all\ninteract:all\npropose:all\nadmin:all\n",
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
            "rngit identity query failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Listening on : "))
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::other("rngit identity query omitted destination"))
}

fn write_python_config(path: &Path, port: u16) -> io::Result<()> {
    fs::create_dir_all(path)?;
    fs::write(
        path.join("config"),
        format!(
            "[reticulum]\nenable_transport = no\nshare_instance = no\n\n[logging]\nloglevel = 0\n\n[interfaces]\n[[TCP Client Interface]]\ntype = TCPClientInterface\nenabled = yes\ntarget_host = 127.0.0.1\ntarget_port = {port}\n"
        ),
    )
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

fn wait_for_port(port: u16, child: &mut Child) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!("rngit exited before listening: {status}")));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "rngit did not open its TCP port"))
}

fn run_python_creator(
    repo: &Path,
    config: &Path,
    identity_path: &Path,
    destination_hex: &str,
    client_number: usize,
) -> io::Result<std::process::Output> {
    const CLIENT: &str = r#"
import json
import sys
import threading
import RNS
from RNS.vendor import umsgpack as mp

config_dir, identity_path, destination_hex, client_number = sys.argv[1:5]
RNS.Reticulum(configdir=config_dir, loglevel=0)
identity = RNS.Identity()
identity.to_file(identity_path)
destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit destination")
remote_identity = RNS.Identity.recall(destination_hash)
if remote_identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(
    remote_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "git", "repositories"
)
ready = threading.Event()
closed_early = []
result = {}
def established(link):
    link.identify(identity)
    ready.set()
def closed(link):
    if not ready.is_set():
        closed_early.append("link closed before activation")
        ready.set()
link = RNS.Link(destination)
link.set_link_established_callback(established)
link.set_link_closed_callback(closed)
if not ready.wait(30):
    raise RuntimeError("rngit link establishment timed out")
if closed_early:
    raise RuntimeError(closed_early[0])
completed = threading.Event()
def response(receipt):
    result["payload"] = receipt.response
    completed.set()
def failed(receipt):
    result["error"] = "work request failed"
    completed.set()
def request(data):
    completed.clear()
    result.clear()
    receipt = link.request(
        "/mgmt/work",
        data,
        response_callback=response,
        failed_callback=failed,
        timeout=30,
    )
    if receipt is False:
        raise RuntimeError("work request was not sent")
    if not completed.wait(30):
        raise RuntimeError("work request timed out")
    if "error" in result:
        raise RuntimeError(result["error"])
    return result["payload"]

unknown_scope_response = request(
    {0: "group/repo", "operation": "list", "scope": "unknown"}
)
if unknown_scope_response[0] != 0:
    raise RuntimeError(
        "pinned Python accepts unknown list scope with success status: "
        + repr(unknown_scope_response)
    )
unknown_scope_result = mp.unpackb(unknown_scope_response[1:])
if unknown_scope_result != {"active": [], "completed": [], "proposed": []}:
    raise RuntimeError(
        "unknown list scope did not return the pinned Python empty scopes map: "
        + repr(unknown_scope_result)
    )

malformed_requests = [
    (
        "malformed document ID",
        {0: "group/repo", "operation": "view", "doc_id": "not-a-number"},
    ),
    ("unknown operation", {0: "group/repo", "operation": "unknown"}),
]
for label, data in malformed_requests:
    payload = request(data)
    if not payload or payload[0] != 2:
        raise RuntimeError(label + " was not rejected as an invalid request: " + repr(payload))

content = "Concurrent Python work body " + client_number
payload = request(
    {
        0: "group/repo",
        "operation": "create",
        "title": "Concurrent Python work " + client_number,
        "content": content,
        "format": "markdown",
        "signature": identity.sign(content.encode("utf-8")),
    }
)
if payload[0] != 0:
    raise RuntimeError("work create response was not successful: " + repr(payload))
document = mp.unpackb(payload[1:])
link.teardown()
print(json.dumps({
    "id": document["id"],
    "scope": document["scope"],
    "unknown_list_scope_accepted": True,
    "malformed_request_cases": len(malformed_requests),
}))
"#;
    Command::new(python_bin())
        .arg("-c")
        .arg(CLIENT)
        .arg(config)
        .arg(identity_path)
        .arg(destination_hex)
        .arg(client_number.to_string())
        .env("PYTHONPATH", repo)
        .output()
}

#[test]
#[ignore = "requires the pinned Python Reticulum checkout"]
fn concurrent_python_clients_reserve_distinct_persisted_work_ids() -> io::Result<()> {
    let _test_guard = PYTHON_RNGIT_INTEROP_LOCK.lock().expect("Python rngit test lock poisoned");
    let python_repo = python_repo();
    if !python_repo.join("RNS/Link.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }
    let temp = tempfile::tempdir()?;
    let root = create_repository_root(temp.path())?;
    let identity_seed = "rngit-concurrent-python-work-server";
    let port = free_port()?;
    let destination = rust_destination(&root, identity_seed)?;
    let mut server = spawn_rngit_server(&root, port, identity_seed)?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let clients = (0..CLIENT_COUNT)
            .map(|index| {
                let config = temp.path().join(format!("python-client-{index}"));
                write_python_config(&config, port)?;
                Ok((index, config.clone(), config.join("identity")))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let outputs = thread::scope(|scope| {
            let handles = clients
                .into_iter()
                .map(|(index, config, identity)| {
                    let repo = python_repo.clone();
                    let destination = destination.clone();
                    scope.spawn(move || {
                        run_python_creator(&repo, &config, &identity, &destination, index)
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().map_err(|_| io::Error::other("Python creator thread panicked"))?
                })
                .collect::<io::Result<Vec<_>>>()
        })?;

        let mut ids = Vec::with_capacity(CLIENT_COUNT);
        for output in outputs {
            if !output.status.success() {
                return Err(io::Error::other(format!(
                    "concurrent Python creator failed: {}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            let result: serde_json::Value =
                serde_json::from_slice(&output.stdout).map_err(|error| {
                    io::Error::other(format!(
                        "Python creator returned invalid JSON: {error}\n{}",
                        String::from_utf8_lossy(&output.stdout)
                    ))
                })?;
            ids.push(
                result
                    .get("id")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| io::Error::other("Python creator omitted numeric work ID"))?,
            );
            if result.get("scope").and_then(serde_json::Value::as_str) != Some("active") {
                return Err(io::Error::other("Python creator did not receive active work scope"));
            }
            if result.get("unknown_list_scope_accepted").and_then(serde_json::Value::as_bool)
                != Some(true)
            {
                return Err(io::Error::other(
                    "Python creator did not verify pinned unknown-list-scope behavior",
                ));
            }
            if result.get("malformed_request_cases").and_then(serde_json::Value::as_u64) != Some(2)
            {
                return Err(io::Error::other(
                    "Python creator did not verify both malformed work request cases",
                ));
            }
        }
        ids.sort_unstable();
        let expected_ids = (1..=CLIENT_COUNT as u64).collect::<Vec<_>>();
        if ids != expected_ids {
            return Err(io::Error::other(format!(
                "concurrent Python clients received IDs {ids:?}, expected {expected_ids:?}"
            )));
        }
        for id in ids {
            let root_path = root.join(format!("group/repo.work/active/{id}/root"));
            if !root_path.is_file() {
                return Err(io::Error::other(format!(
                    "work ID {id} was returned but its document was not persisted"
                )));
            }
        }
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

#[cfg(unix)]
fn write_cli_editor(path: &Path, content: &str) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let script = format!(
        "#!/usr/bin/env python3\nfrom pathlib import Path\nimport sys\nPath(sys.argv[1]).write_text({content:?}, encoding=\"utf-8\")\n"
    );
    fs::write(path, script)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(unix)]
struct PythonWorkCli<'a> {
    repo: &'a Path,
    config: &'a Path,
    identity: &'a Path,
    editor: &'a Path,
    remote: &'a str,
}

#[cfg(unix)]
impl PythonWorkCli<'_> {
    fn run(
        &self,
        options: &[&str],
        operation: &str,
        confirmation: Option<&str>,
    ) -> io::Result<std::process::Output> {
        let mut command = Command::new(python_bin());
        command
            .arg(self.repo.join("RNS/Utilities/rngit/server.py"))
            .arg("work")
            .arg("--config")
            .arg(self.config)
            .arg("--rnsconfig")
            .arg(self.config)
            .arg("--identity")
            .arg(self.identity)
            .args(options)
            .arg(self.remote)
            .arg(operation)
            .env("PYTHONPATH", self.repo)
            .env("EDITOR", self.editor)
            .stdin(if confirmation.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        if let Some(confirmation) = confirmation {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(confirmation.as_bytes())?;
            }
        }
        child.wait_with_output()
    }
}

#[cfg(unix)]
fn assert_python_cli_output(output: &std::process::Output, expected: &str) -> io::Result<()> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() && stdout.contains(expected) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "pinned Python rngit CLI did not report {expected:?}: {}\nstdout:\n{stdout}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

#[cfg(unix)]
#[test]
#[ignore = "requires the pinned Python Reticulum checkout"]
fn pinned_python_rngit_work_cli_round_trips_production_service_lifecycle() -> io::Result<()> {
    let _test_guard = PYTHON_RNGIT_INTEROP_LOCK.lock().expect("Python rngit test lock poisoned");
    let python_repo = python_repo();
    if !python_repo.join("RNS/Utilities/rngit/server.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python rngit checkout not found: {}", python_repo.display()),
        ));
    }

    let temp = tempfile::tempdir()?;
    let root = create_repository_root(temp.path())?;
    let identity_seed = "rngit-python-cli-work-server";
    let port = free_port()?;
    let destination = rust_destination(&root, identity_seed)?;
    let mut server = spawn_rngit_server(&root, port, identity_seed)?;
    let result = (|| {
        wait_for_port(port, &mut server)?;
        let config = temp.path().join("python-cli-client");
        write_python_config(&config, port)?;
        let identity = config.join("rngit-identity");
        let editor = temp.path().join("rngit-test-editor.py");
        let remote = format!("rns://{destination}/group/repo");
        let cli = PythonWorkCli {
            repo: &python_repo,
            config: &config,
            identity: &identity,
            editor: &editor,
            remote: &remote,
        };

        write_cli_editor(&editor, "CLI-created work body")?;
        let created = cli.run(&["--title", "CLI work item"], "create", None)?;
        assert_python_cli_output(&created, "created as active #1")?;

        let listed = cli.run(&["--scope", "active"], "list", None)?;
        assert_python_cli_output(&listed, "CLI work item")?;

        let viewed = cli.run(&["--id", "1"], "view", None)?;
        assert_python_cli_output(&viewed, "CLI-created work body")?;
        assert_python_cli_output(&viewed, "Signature : Valid")?;

        write_cli_editor(&editor, "CLI-edited work body")?;
        let edited = cli.run(&["--title", "CLI edited item", "--id", "1"], "edit", None)?;
        assert_python_cli_output(&edited, "updated")?;

        write_cli_editor(&editor, "CLI comment body")?;
        let commented = cli.run(&["--id", "1"], "update", None)?;
        assert_python_cli_output(&commented, "Update #1 added")?;

        write_cli_editor(&editor, "read:all\nwrite:all\ninteract:all\nadmin:all\n")?;
        let permissions = cli.run(&["--id", "1"], "perms", None)?;
        assert_python_cli_output(&permissions, "Permissions updated for work document #1")?;
        assert_eq!(
            fs::read_to_string(root.join("group/repo.work/1.allowed"))?,
            "read:all\nwrite:all\ninteract:all\nadmin:all\n"
        );

        // Exercise a denied edit through the pinned Python CLI and the live
        // production Link. Keep admin access so the client can restore policy
        // after proving that explicit write denial takes effect.
        write_cli_editor(&editor, "read:all\nwrite:none\ninteract:all\nadmin:all\n")?;
        let denied_permissions = cli.run(&["--id", "1"], "perms", None)?;
        assert_python_cli_output(&denied_permissions, "Permissions updated for work document #1")?;
        let document_path = root.join("group/repo.work/active/1/root");
        let persisted_before_denied_edit = fs::read(&document_path)?;

        write_cli_editor(&editor, "Unauthorized Python edit must not persist")?;
        let denied_edit = cli.run(&["--title", "Unauthorized edit", "--id", "1"], "edit", None)?;
        let denied_output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&denied_edit.stdout),
            String::from_utf8_lossy(&denied_edit.stderr)
        );
        if denied_edit.status.success() || !denied_output.contains("No access, not author") {
            return Err(io::Error::other(format!(
                "pinned Python CLI did not report the denied edit: {}\n{denied_output}",
                denied_edit.status
            )));
        }
        if fs::read(&document_path)? != persisted_before_denied_edit {
            return Err(io::Error::other("denied Python edit changed the persisted work document"));
        }
        let unchanged = cli.run(&["--id", "1"], "view", None)?;
        assert_python_cli_output(&unchanged, "CLI-edited work body")?;
        if String::from_utf8_lossy(&unchanged.stdout)
            .contains("Unauthorized Python edit must not persist")
        {
            return Err(io::Error::other("denied Python edit changed the in-memory work document"));
        }

        write_cli_editor(&editor, "read:all\nwrite:all\ninteract:all\nadmin:all\n")?;
        let restored_permissions = cli.run(&["--id", "1"], "perms", None)?;
        assert_python_cli_output(
            &restored_permissions,
            "Permissions updated for work document #1",
        )?;

        let completed = cli.run(&["--id", "1"], "complete", None)?;
        assert_python_cli_output(&completed, "Work document #1 completed")?;
        let completed_list = cli.run(&["--scope", "completed"], "list", None)?;
        assert_python_cli_output(&completed_list, "CLI edited item")?;

        let activated = cli.run(&["--id", "1"], "activate", None)?;
        assert_python_cli_output(&activated, "Work document #1 activated")?;

        write_cli_editor(&editor, "CLI proposal body")?;
        let proposed = cli.run(&["--title", "CLI proposal"], "propose", None)?;
        assert_python_cli_output(&proposed, "created as proposed #2")?;
        let proposed_list = cli.run(&["--scope", "proposed"], "list", None)?;
        assert_python_cli_output(&proposed_list, "CLI proposal")?;

        let root_document = root.join("group/repo.work/active/1/root");
        let original_document = fs::read(&root_document)?;
        fs::write(&root_document, [0x80, 0xc1])?;
        let malformed_view = cli.run(&["--id", "1"], "view", None)?;
        let malformed_output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&malformed_view.stdout),
            String::from_utf8_lossy(&malformed_view.stderr)
        );
        if malformed_view.status.success()
            || !malformed_output.contains("Remote error: Error loading document")
        {
            return Err(io::Error::other(format!(
                "pinned Python CLI did not receive the persisted-document failure: {}\n{malformed_output}",
                malformed_view.status
            )));
        }
        fs::write(&root_document, original_document)?;

        let deleted = cli.run(&["--id", "1"], "delete", Some("y\n"))?;
        assert_python_cli_output(&deleted, "Work document active #1 deleted")?;
        let deleted_proposal =
            cli.run(&["--scope", "proposed", "--id", "2"], "delete", Some("y\n"))?;
        assert_python_cli_output(&deleted_proposal, "Work document proposed #2 deleted")?;
        if root.join("group/repo.work/active/1/root").exists()
            || root.join("group/repo.work/proposed/2/root").exists()
        {
            return Err(io::Error::other("Python CLI delete left work document files"));
        }
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}
