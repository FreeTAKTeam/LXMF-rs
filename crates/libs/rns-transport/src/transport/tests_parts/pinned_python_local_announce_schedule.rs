#[tokio::test]
#[ignore = "requires pinned Python Reticulum checkout at RETICULUM_PY_REPO"]
async fn pinned_python_local_client_schedule_matches_production_worker_tick() {
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
    let values: Vec<&str> = constants
        .trim()
        .split(',')
        .take(3)
        .collect();
    let numeric_values: Vec<u64> = values
        .iter()
        .map(|value| value.parse().expect("Python schedule value is an integer"))
        .collect();
    assert_eq!(numeric_values, [1, 1_000, 250]);
    assert!(constants.contains(",strict-boundary,one-retransmit,"));
    let python_poll_bound = Duration::from_millis(numeric_values[1] + numeric_values[2]);
    assert_eq!(python_poll_bound, PYTHON_LOCAL_ANNOUNCE_POLL_BOUND);
    assert!(
        INTERVAL_ANNOUNCES_RETRANSMIT < python_poll_bound,
        "Rust's first 1.0 s worker tick is inside Python's source-derived 1.0 s check + 0.25 s poll bound"
    );

    let python_observations: Vec<usize> = constants
        .trim()
        .split(',')
        .skip(5)
        .map(|value| value.parse().expect("Python observation is a packet count"))
        .collect();
    assert_eq!(
        python_observations,
        rust_local_client_announce_schedule().await,
        "production Rust tick packet counts must match the pinned Python announce job"
    );
}
