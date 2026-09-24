use std::net::TcpListener;
use std::process::Command;

#[test]
fn rnpath_reports_daemon_unavailable_without_claiming_path_success() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve an unused RPC endpoint");
    let address = listener.local_addr().expect("read reserved RPC endpoint");
    drop(listener);

    let output = Command::new(env!("CARGO_BIN_EXE_rnpath-rs"))
        .args(["00112233445566778899aabbccddeeff", "--rpc", &address.to_string(), "--timeout", "1"])
        .output()
        .expect("run production rnpath CLI");

    assert!(!output.status.success(), "rnpath unexpectedly succeeded");
    assert!(
        output.stdout.is_empty(),
        "rnpath emitted apparent success: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("connect"),
        "missing daemon connection context: {stderr}"
    );
    assert!(stderr.contains("Connection refused"), "missing daemon connection failure: {stderr}");
}
