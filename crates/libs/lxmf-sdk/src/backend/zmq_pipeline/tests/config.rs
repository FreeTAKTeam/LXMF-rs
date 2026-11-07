use super::*;

#[cfg(windows)]
#[test]
fn config_normalizes_ipv4_loopback_for_windows_tcp_bind() {
    let config =
        ZmqPipelineBackendConfig::local_tcp("tcp://127.0.0.1:9000", "tcp://127.0.0.1:9001");
    assert_eq!(config.command_endpoint, "tcp://localhost:9000");
    assert_eq!(config.response_endpoint, "tcp://localhost:9001");
    assert_eq!(
        ZmqPipelineBackendConfig::local("tcp://127.0.0.1:9000").command_endpoint,
        "tcp://localhost:9000"
    );
}

#[cfg(not(windows))]
#[test]
fn config_preserves_explicit_loopback_address_family_outside_windows() {
    for (command, response) in [
        ("tcp://127.0.0.1:9000", "tcp://127.0.0.1:9001"),
        ("tcp://[::1]:9000", "tcp://[::1]:9001"),
        ("tcp://localhost:9000", "tcp://localhost:9001"),
    ] {
        let config = ZmqPipelineBackendConfig::local_tcp(command, response);
        assert_eq!(config.command_endpoint, command);
        assert_eq!(config.response_endpoint, response);
        assert_eq!(config.request_timeout, std::time::Duration::from_secs(5));
        config.validate().expect("loopback remains local-trusted");
        assert_eq!(ZmqPipelineBackendConfig::local(command).command_endpoint, command);
        let authenticated = ZmqPipelineBackendConfig::remote(
            command,
            ZmqPipelineTokenAuth {
                issuer: "issuer".into(),
                audience: "audience".into(),
                shared_secret: "test-secret".into(),
                ttl_secs: 60,
            },
        );
        assert_eq!(authenticated.command_endpoint, command);
    }
}
