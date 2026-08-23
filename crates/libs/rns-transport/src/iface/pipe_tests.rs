use super::PipeInterface;
use std::time::Duration;

#[test]
fn pipe_command_parser_matches_python_shlex_baseline() {
    let argv = PipeInterface::parse_command("prog --flag 'two words'").expect("parse");
    assert_eq!(argv, vec!["prog", "--flag", "two words"]);
    assert!(PipeInterface::parse_command("'unterminated").is_err());
    assert!(PipeInterface::parse_command("").is_err());
}

#[test]
fn pipe_builder_exposes_defaults_and_overrides() {
    let adapter =
        PipeInterface::new("cat").with_respawn_delay(Duration::from_millis(250)).with_mtu(512);
    assert_eq!(adapter.command(), "cat");
    assert_eq!(adapter.mtu_value(), 512);
    let status = adapter.runtime_status_json();
    assert_eq!(status["command"].as_str(), Some("cat"));
    assert_eq!(status["process_state"].as_str(), Some("configured"));
    assert_eq!(status["pipe_is_open"].as_bool(), Some(false));
    assert_eq!(status["respawn_attempts"].as_u64(), Some(0));
    assert!(status["last_error"].is_null());
}

#[test]
fn pipe_runtime_status_handle_records_respawn_errors() {
    let adapter = PipeInterface::new("cat");
    let status = adapter.runtime_status_handle();

    status.record_error_for_test("respawning", "spawn cat failed");

    let json = status.to_json();
    assert_eq!(json["command"].as_str(), Some("cat"));
    assert_eq!(json["process_state"].as_str(), Some("respawning"));
    assert_eq!(json["pipe_is_open"].as_bool(), Some(false));
    assert_eq!(json["respawn_attempts"].as_u64(), Some(1));
    assert_eq!(json["last_error"].as_str(), Some("spawn cat failed"));
}
