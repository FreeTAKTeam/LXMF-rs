#[test]
#[ignore = "requires the pinned Python Reticulum reference"]
fn rngit_work_view_preserves_explicit_nil_optional_signature() {
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::Stdio;

    const PYTHON_REFERENCE_REVISION: &str = "99de23c040d507e3fefca19e87b182302902725d";
    let reference = std::env::var_os("RETICULUM_PY_REPO")
        .map(PathBuf::from)
        .expect("RETICULUM_PY_REPO must point to the pinned Python checkout");
    let revision = Command::new("git")
        .arg("-C")
        .arg(&reference)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("read Python reference revision");
    assert!(revision.status.success(), "could not resolve Python reference HEAD");
    assert_eq!(String::from_utf8_lossy(&revision.stdout).trim(), PYTHON_REFERENCE_REVISION);

    let temp = tempfile::tempdir().expect("tempdir");
    let group = temp.path().join("group");
    let repository = group.join("repo");
    fs::create_dir_all(&repository).expect("repository directory");
    assert!(Command::new("git")
        .args(["init", "--bare", repository.to_string_lossy().as_ref()])
        .status()
        .expect("git init")
        .success());
    fs::write(group.join("repo.allowed"), "read:all\n").expect("permissions");
    let work_root = group.join("repo.work/active/73");
    fs::create_dir_all(&work_root).expect("work directory");
    let document = rmpv::Value::Map(vec![
        (rmpv::Value::String("content".into()), rmpv::Value::String("wire fixture".into())),
        (
            rmpv::Value::String("meta".into()),
            rmpv::Value::Map(vec![
                (rmpv::Value::String("title".into()), rmpv::Value::String("Nil signature".into())),
                (rmpv::Value::String("created".into()), rmpv::Value::from(1_700_000_000_u64)),
                (rmpv::Value::String("edited".into()), rmpv::Value::from(1_700_000_000_u64)),
                (rmpv::Value::String("author".into()), rmpv::Value::Binary(vec![0x42; 16])),
                (rmpv::Value::String("signature".into()), rmpv::Value::Nil),
            ]),
        ),
    ]);
    let mut stored = Vec::new();
    rmpv::encode::write_value(&mut stored, &document).expect("encode work fixture");
    fs::write(work_root.join("root"), stored).expect("write work fixture");

    let mut node = ReticulumGitNode::default();
    node.load_repository_group("group", &group).expect("load production repository group");
    let request = crate::pack_request([
        (rmpv::Value::from(0_u64), rmpv::Value::String("group/repo".into())),
        (rmpv::Value::String("operation".into()), rmpv::Value::String("view".into())),
        (rmpv::Value::String("doc_id".into()), rmpv::Value::from(73_u64)),
        (rmpv::Value::String("scope".into()), rmpv::Value::String("active".into())),
    ])
    .expect("encode production request");
    let response = node.handle_request("/mgmt/work", &request, [0x24; 16]);
    assert_eq!(response.first(), Some(&ReticulumGitNode::RES_OK));

    let python = std::env::var_os("LXMF_PYTHON_BIN").unwrap_or_else(|| "python3".into());
    let mut child = Command::new(python)
        .env("PYTHONPATH", &reference)
        .arg("-c")
        .arg("import msgpack,sys; from RNS.Utilities.rngit import server; value=msgpack.unpackb(sys.stdin.buffer.read(), raw=False); assert value['meta']['signature'] is None; print('nil-signature-ok')")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start frozen Python MessagePack decoder");
    child.stdin.take().expect("Python stdin").write_all(&response[1..]).expect("send production response");
    let output = child.wait_with_output().expect("wait for Python decoder");
    assert!(output.status.success(), "Python wire check failed: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "nil-signature-ok");
}
