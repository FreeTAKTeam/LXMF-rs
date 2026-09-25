const PAGE_GIT_OVERFLOW_CHILD_ENV: &str = "RNGIT_PAGE_GIT_OVERFLOW_CHILD";
const PAGE_GIT_OVERFLOW_CHILD_MARKER: &str = "rngit-page-git-overflow-fixture-v1";

#[test]
#[ignore = "subprocess fixture for page_git_output_rejects_overflow_and_reaps_child_promptly"]
fn page_git_output_overflow_fixture_child() {
    if std::env::var_os(PAGE_GIT_OVERFLOW_CHILD_ENV).as_deref()
        != Some(std::ffi::OsStr::new(PAGE_GIT_OVERFLOW_CHILD_MARKER))
    {
        return;
    }
    use std::io::Write as _;
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let chunk = [b'x'; 64 * 1024];
    loop {
        if writer.write_all(&chunk).is_err() || writer.flush().is_err() {
            return;
        }
    }
}

#[test]
fn page_git_output_rejects_overflow_and_reaps_child_promptly() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let executable = std::env::current_exe().expect("test executable path");
    let mut child = std::process::Command::new(executable)
        .args([
            "--exact",
            "tests::page_git_output_overflow_fixture_child",
            "--ignored",
            "--nocapture",
        ])
        .env(PAGE_GIT_OVERFLOW_CHILD_ENV, PAGE_GIT_OVERFLOW_CHILD_MARKER)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn unbounded-output fixture");
    let started = Instant::now();

    assert!(super::page_git_output::read_bounded_child_stdout(&mut child, 1024).is_none());
    assert!(started.elapsed() < Duration::from_secs(5), "overflow child was not stopped promptly");
    assert!(child.try_wait().expect("child wait status").is_some(), "child was not reaped");
}
