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
