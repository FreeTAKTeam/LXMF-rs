use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CLIENT_COUNT: usize = 4;

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
content = "Concurrent Python work body " + client_number
receipt = link.request(
    "/mgmt/work",
    {
        0: "group/repo",
        "operation": "create",
        "title": "Concurrent Python work " + client_number,
        "content": content,
        "format": "markdown",
        "signature": identity.sign(content.encode("utf-8")),
    },
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
payload = result["payload"]
if payload[0] != 0:
    raise RuntimeError("work create response was not successful: " + repr(payload))
document = mp.unpackb(payload[1:])
link.teardown()
print(json.dumps({"id": document["id"], "scope": document["scope"]}))
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
