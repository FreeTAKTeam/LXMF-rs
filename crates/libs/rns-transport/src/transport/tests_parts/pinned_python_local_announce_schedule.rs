#[test]
#[ignore = "requires pinned Python Reticulum checkout at RETICULUM_PY_REPO"]
fn pinned_python_local_client_schedule_bounds_the_rust_worker_tick() {
    const PINNED_RETICULUM: &str = "99de23c040d507e3fefca19e87b182302902725d";
    let python_repo = std::env::var("RETICULUM_PY_REPO")
        .expect("set RETICULUM_PY_REPO to the pinned Python Reticulum checkout");
    let revision = std::process::Command::new("git")
        .args(["-C", &python_repo, "rev-parse", "HEAD"])
        .output()
        .expect("read pinned Python Reticulum revision");
    assert!(revision.status.success(), "git rev-parse failed for {python_repo}");
    assert_eq!(
        String::from_utf8_lossy(&revision.stdout).trim(),
        PINNED_RETICULUM,
        "schedule contract must be read from the issue's pinned Python reference"
    );

    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let script = include_str!("../../../tests/support/pinned_local_announce_schedule.py");
    let output = std::process::Command::new(python)
        .args(["-c", script])
        .env(
            "PYTHONPATH",
            format!("{python_repo}:{}", std::env::var("PYTHONPATH").unwrap_or_default()),
        )
        .output()
        .expect("run pinned Python local-client schedule differential");
    assert!(
        output.status.success(),
        "pinned Python schedule differential failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let constants = String::from_utf8_lossy(&output.stdout);
    assert!(constants.contains("strict-boundary,one-retransmit"));
    let values: Vec<u64> = constants
        .split(',')
        .take(3)
        .map(|value| value.parse().expect("Python schedule value is an integer"))
        .collect();
    assert_eq!(values, [1, 1_000, 250]);
    let python_poll_bound = Duration::from_millis(values[1] + values[2]);
    assert_eq!(python_poll_bound, PYTHON_LOCAL_ANNOUNCE_POLL_BOUND);
    assert!(
        INTERVAL_ANNOUNCES_RETRANSMIT < python_poll_bound,
        "Rust's first 1.0 s worker tick is inside Python's source-derived 1.0 s check + 0.25 s poll bound"
    );
}
