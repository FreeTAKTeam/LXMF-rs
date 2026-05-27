use std::fs;
use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use tokio::time::{sleep, Instant};

static PYTHON_INTEROP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) struct PythonChannelInteropPaths {
    python_bin: String,
    reticulum_py_repo: PathBuf,
    helper: PathBuf,
}

impl PythonChannelInteropPaths {
    pub(super) fn spawn_endpoint(&self, config_dir: &Path, payload_kind: &str) -> Child {
        spawn_python_endpoint(
            &self.python_bin,
            &self.reticulum_py_repo,
            &self.helper,
            config_dir,
            payload_kind,
        )
    }

    pub(super) fn spawn_channel_client(
        &self,
        config_dir: &Path,
        destination_hash: &str,
        payload_kind: &str,
    ) -> Child {
        spawn_python_channel_client(
            &self.python_bin,
            &self.reticulum_py_repo,
            &self.helper,
            config_dir,
            destination_hash,
            payload_kind,
        )
    }
}

pub(super) struct ChildGuard {
    pub(super) child: Option<Child>,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(serde::Deserialize)]
pub(super) struct ReadyLine {
    pub(super) ready: bool,
    pub(super) destination_hash: String,
}

pub(super) async fn python_interop_guard() -> tokio::sync::MutexGuard<'static, ()> {
    PYTHON_INTEROP_LOCK.lock().await
}

pub(super) fn python_channel_interop_paths() -> PythonChannelInteropPaths {
    let python_bin = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulum_py_repo = std::env::var("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("../reticulum"));
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_channel_endpoint.py");

    PythonChannelInteropPaths { python_bin, reticulum_py_repo, helper }
}

pub(super) fn spawn_python_endpoint(
    python_bin: &str,
    reticulum_py_repo: &Path,
    helper: &Path,
    config_dir: &Path,
    payload_kind: &str,
) -> Child {
    Command::new(python_bin)
        .arg("-u")
        .arg(helper)
        .arg("--payload-kind")
        .arg(payload_kind)
        .arg("--config-dir")
        .arg(config_dir)
        .env("PYTHONPATH", reticulum_py_repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn python endpoint")
}

pub(super) fn spawn_python_channel_client(
    python_bin: &str,
    reticulum_py_repo: &Path,
    helper: &Path,
    config_dir: &Path,
    destination_hash: &str,
    payload_kind: &str,
) -> Child {
    Command::new(python_bin)
        .arg("-u")
        .arg(helper)
        .arg("--mode")
        .arg("client")
        .arg("--payload-kind")
        .arg(payload_kind)
        .arg("--config-dir")
        .arg(config_dir)
        .arg("--destination-hash")
        .arg(destination_hash)
        .arg("--message-id")
        .arg("python-1")
        .arg("--message-data")
        .arg("hello-rust")
        .env("PYTHONPATH", reticulum_py_repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn python channel client")
}

pub(super) fn read_ready(child: &mut Child) -> Option<ReadyLine> {
    let stdout = child.stdout.take().expect("python stdout");
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let line = line.expect("read ready line");
        if let Ok(ready) = serde_json::from_str::<ReadyLine>(&line) {
            if ready.ready {
                return Some(ready);
            }
        }
    }
    None
}

pub(super) async fn wait_for_port(port: u16, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for Python TCP server on port {port}");
}

pub(super) fn write_python_config(dir: &Path, port: u16) {
    let config = format!(
        "[reticulum]\n\
         enable_transport = no\n\
         share_instance = no\n\
         \n\
         [logging]\n\
         loglevel = 7\n\
         \n\
         [interfaces]\n\
           [[TCP Server Interface]]\n\
             type = TCPServerInterface\n\
             enabled = yes\n\
             listen_ip = 127.0.0.1\n\
             listen_port = {port}\n"
    );
    fs::write(dir.join("config"), config).expect("write python config");
}

pub(super) fn write_python_client_config(dir: &Path, port: u16) {
    let config = format!(
        "[reticulum]\n\
         enable_transport = no\n\
         share_instance = no\n\
         \n\
         [logging]\n\
         loglevel = 7\n\
         \n\
         [interfaces]\n\
           [[TCP Client Interface]]\n\
             type = TCPClientInterface\n\
             enabled = yes\n\
             target_host = 127.0.0.1\n\
             target_port = {port}\n"
    );
    fs::write(dir.join("config"), config).expect("write python client config");
}

pub(super) fn free_tcp_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}
