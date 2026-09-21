use std::io::BufRead;
use std::fs as test_fs;
use std::io as test_io;
use std::path::PathBuf as TestPathBuf;
use std::process::{Command as TestCommand, Stdio as TestStdio};

#[test]
#[ignore = "requires local Python Reticulum checkout"]
fn native_rust_client_requests_pinned_python_rngit() -> test_io::Result<()> {
    let python_repo = std::env::var_os("RETICULUM_PY_REPO")
        .map(TestPathBuf::from)
        .unwrap_or_else(|| TestPathBuf::from(".tmp/python-refs/Reticulum"));
    let python_repo = if python_repo.is_absolute() {
        python_repo
    } else {
        TestPathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").join(python_repo)
    };
    if !python_repo.join("RNS/Utilities/rngit/server.py").is_file() {
        return Err(test_io::Error::new(
            test_io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }

    let temp = tempfile::tempdir()?;
    let root = temp.path().join("python-rngit-root");
    let group = root.join("group");
    let repository = group.join("repo");
    let source = temp.path().join("git-source");
    test_fs::create_dir_all(&group)?;
    TestCommand::new("git")
        .args(["init", "-q", source.to_string_lossy().as_ref()])
        .status()?
        .success()
        .then_some(())
        .ok_or_else(|| test_io::Error::other("could not create Python rngit source repository"))?;
    TestCommand::new("git")
        .current_dir(&source)
        .args(["checkout", "-q", "-b", "main"])
        .status()?
        .success()
        .then_some(())
        .ok_or_else(|| test_io::Error::other("could not create Python rngit main branch"))?;
    for (key, value) in [("user.email", "rngit-native@example.invalid"), ("user.name", "rngit-native")]
    {
        TestCommand::new("git")
            .current_dir(&source)
            .args(["config", key, value])
            .status()?
            .success()
            .then_some(())
            .ok_or_else(|| test_io::Error::other("could not configure Python rngit source"))?;
    }
    test_fs::write(source.join("README.md"), "native client fetch fixture\n")?;
    TestCommand::new("git")
        .current_dir(&source)
        .args(["add", "README.md"])
        .status()?
        .success()
        .then_some(())
        .ok_or_else(|| test_io::Error::other("could not stage Python rngit source"))?;
    TestCommand::new("git")
        .current_dir(&source)
        .args(["commit", "-q", "-m", "native client fetch fixture"])
        .status()?
        .success()
        .then_some(())
        .ok_or_else(|| test_io::Error::other("could not commit Python rngit source"))?;
    TestCommand::new("git")
        .args(["clone", "-q", "--bare", source.to_string_lossy().as_ref(), repository.to_string_lossy().as_ref()])
        .status()?
        .success()
        .then_some(())
        .ok_or_else(|| test_io::Error::other("could not create Python rngit bare repository"))?;
    test_fs::write(
        root.join("group.allowed"),
        "read:all\nwrite:all\ncreate:all\ninteract:all\nadmin:all\n",
    )?;
    test_fs::write(repository.with_extension("allowed"), "read:all\nwrite:all\ncreate:all\n")?;

    let port = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let rns_config = temp.path().join("python-rns");
    let rngit_config = temp.path().join("python-rngit");
    test_fs::create_dir_all(&rns_config)?;
    test_fs::create_dir_all(&rngit_config)?;
    test_fs::write(
        rns_config.join("config"),
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
    )?;
    test_fs::write(
        rngit_config.join("config"),
        format!(
            "[logging]\n\
             loglevel = 0\n\
             \n\
             [repositories]\n\
             group = {}\n",
            group.display()
        ),
    )?;

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
    let mut server = TestCommand::new(std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string()))
        .arg("-c")
        .arg(server_script)
        .arg(&rngit_config)
        .arg(&rns_config)
        .env("PYTHONPATH", &python_repo)
        .stdout(TestStdio::piped())
        .stderr(TestStdio::piped())
        .spawn()?;
    let destination = {
        let stdout = server
            .stdout
            .take()
            .ok_or_else(|| test_io::Error::other("Python rngit stdout was not captured"))?;
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        line.trim().to_string()
    };
    let result = (|| {
        let destination = hex::decode(destination)
            .map_err(|error| test_io::Error::other(format!("invalid Python destination: {error}")))?;
        let destination: [u8; 16] = destination
            .try_into()
            .map_err(|_| test_io::Error::other("Python destination was not 16 bytes"))?;
        let identity = rns_transport::identity::PrivateIdentity::new_from_name("rngit-native-rust-client");
        let mut client = ReticulumGitClient::default();
        client.attach_native_tcp(identity, format!("127.0.0.1:{port}"));
        let remote = format!("rns://{}/group/repo", hex::encode(destination));

        let listing = client.handle_git_list(&remote, false).map_err(test_io::Error::other)?;
        if !listing.iter().any(|reference| reference.contains("HEAD")) {
            return Err(test_io::Error::other(format!("initial Git listing omitted HEAD: {listing:?}")));
        }

        let fetched = client
            .process_fetch_queue(&remote, &["refs/heads/main".to_string()])
            .map_err(test_io::Error::other)?;
        if fetched.first().copied() != Some(0) || fetched.len() <= 1 {
            return Err(test_io::Error::other(format!("Python Git fetch failed: {fetched:?}")));
        }

        let created = client
            .work_create(&remote, "Rust native request", &"R".repeat(4096))
            .map_err(test_io::Error::other)?;
        if created.first().copied() != Some(0) {
            return Err(test_io::Error::other(format!("Python work create failed: {created:?}")));
        }
        let listed = client.work_list(&remote, "active").map_err(test_io::Error::other)?;
        if listed.first().copied() != Some(0) {
            return Err(test_io::Error::other(format!("Python work list failed: {listed:?}")));
        }
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}
