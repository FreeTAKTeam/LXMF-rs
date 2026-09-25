use super::{ReticulumGitNode, DEFAULT_BASE_TEMPLATE};
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn failed_executable_template_falls_back_to_builtin_template() {
    let temporary = tempfile::tempdir().expect("temporary template directory");
    let template = temporary.path().join("base.mu");
    fs::write(&template, "#!/definitely/missing/rngit-template-interpreter\n")
        .expect("write unlaunchable executable template");
    let mut permissions = fs::metadata(&template).expect("template metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&template, permissions).expect("make template executable");

    let mut node = ReticulumGitNode::default();
    assert_eq!(node.load_page_templates(temporary.path()).expect("template failure is non-fatal"), 0);
    assert_eq!(node.template("base"), DEFAULT_BASE_TEMPLATE);
}

#[cfg(target_os = "linux")]
#[test]
fn executable_template_with_unlaunchable_interpreter_falls_back_to_builtin_template() {
    let temporary = tempfile::tempdir().expect("temporary template directory");
    let interpreter = temporary.path().join("non-executable-interpreter");
    fs::write(&interpreter, "#!/bin/sh\nprintf 'unexpected override'\n")
        .expect("write interpreter fixture");
    let mut permissions = fs::metadata(&interpreter).expect("interpreter metadata").permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&interpreter, permissions).expect("remove interpreter execute bits");

    let template = temporary.path().join("base.mu");
    fs::write(&template, format!("#!{}\n", interpreter.display()))
        .expect("write denied-interpreter template");
    let mut permissions = fs::metadata(&template).expect("template metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&template, permissions).expect("make template executable");

    let mut node = ReticulumGitNode::default();
    assert_eq!(node.load_page_templates(temporary.path()).expect("launch failure is non-fatal"), 0);
    assert_eq!(node.template("base"), DEFAULT_BASE_TEMPLATE);
}

#[cfg(unix)]
#[test]
fn executable_template_uses_stdout_when_process_exits_nonzero() {
    let temporary = tempfile::tempdir().expect("temporary template directory");
    let template = temporary.path().join("base.mu");
    fs::write(&template, "#!/bin/sh\nprintf 'dynamic template'\nexit 7\n")
        .expect("write nonzero executable template");
    let mut permissions = fs::metadata(&template).expect("template metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&template, permissions).expect("make template executable");

    let mut node = ReticulumGitNode::default();
    assert_eq!(node.load_page_templates(temporary.path()).expect("nonzero status is not a Python exception"), 1);
    assert_eq!(node.template("base"), "dynamic template");
}

#[cfg(unix)]
#[test]
fn executable_template_does_not_wait_for_descendant_holding_output_pipes() {
    let temporary = tempfile::tempdir().expect("temporary template directory");
    let template = temporary.path().join("base.mu");
    fs::write(
        &template,
        "#!/bin/sh\nsleep 5 &\nprintf 'partial template'\nexit 0\n",
    )
    .expect("write template with pipe-inheriting descendant");
    let mut permissions = fs::metadata(&template).expect("template metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&template, permissions).expect("make template executable");

    let start = std::time::Instant::now();
    let mut node = ReticulumGitNode::default();
    let loaded = node.load_page_templates(temporary.path()).expect("template failure is non-fatal");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(3),
        "template descendant kept the loader blocked for {:?}",
        start.elapsed()
    );
    assert_eq!(loaded, 0);
    assert_eq!(node.template("base"), DEFAULT_BASE_TEMPLATE);
}

#[cfg(target_os = "linux")]
#[test]
fn executable_template_does_not_wait_for_descendant_that_escapes_process_group() {
    let temporary = tempfile::tempdir().expect("temporary template directory");
    let template = temporary.path().join("base.mu");
    fs::write(
        &template,
        "#!/bin/sh\nsetsid sleep 4 &\nprintf 'partial template'\nexit 0\n",
    )
    .expect("write template with session-escaping descendant");
    let mut permissions = fs::metadata(&template).expect("template metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&template, permissions).expect("make template executable");

    let start = std::time::Instant::now();
    let mut node = ReticulumGitNode::default();
    let loaded = node.load_page_templates(temporary.path()).expect("template failure is non-fatal");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(3),
        "template descendant kept the loader blocked for {:?}",
        start.elapsed()
    );
    assert_eq!(loaded, 0);
    assert_eq!(node.template("base"), DEFAULT_BASE_TEMPLATE);
}
