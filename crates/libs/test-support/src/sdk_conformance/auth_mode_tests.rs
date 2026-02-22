use super::{
    insecure_remote_start_request, mtls_remote_start_request, token_remote_start_request,
    token_without_config_start_request, RpcHarness,
};
use lxmf_sdk::LxmfSdk;

#[test]
fn sdk_conformance_remote_bind_requires_secure_auth_mode() {
    let harness = RpcHarness::new();
    let client = harness.client();

    let err = client
        .start(insecure_remote_start_request())
        .expect_err("remote bind without token/mtls must fail");
    assert_eq!(err.machine_code, "SDK_SECURITY_REMOTE_BIND_DISALLOWED");
}

#[test]
fn sdk_conformance_token_mode_requires_token_config() {
    let harness = RpcHarness::new();
    let client = harness.client();

    let err = client
        .start(token_without_config_start_request())
        .expect_err("token mode requires token config");
    assert_eq!(err.machine_code, "SDK_SECURITY_AUTH_REQUIRED");
}

#[test]
fn sdk_conformance_token_mode_supports_multiple_authenticated_rpc_calls() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client
        .start(token_remote_start_request())
        .expect("token-mode start with config should succeed");

    let first = client.snapshot().expect("first snapshot");
    let second = client.snapshot().expect("second snapshot");
    assert_eq!(first.runtime_id, second.runtime_id);
}

#[test]
fn sdk_conformance_mtls_mode_requires_client_material_when_certs_are_mandatory() {
    let harness = RpcHarness::new();
    let client = harness.client();
    let err = client
        .start(mtls_remote_start_request())
        .expect_err("mtls mode with require_client_cert=true must require client cert paths");
    assert_eq!(err.machine_code, "SDK_SECURITY_AUTH_REQUIRED");
}

#[test]
fn sdk_conformance_shared_instance_capability_is_negotiated_when_requested() {
    let harness = RpcHarness::new();
    let client = harness.client();
    let mut request = token_remote_start_request();
    request.requested_capabilities = vec!["sdk.capability.shared_instance_rpc_auth".to_string()];

    let handle = client.start(request).expect("start with shared-instance capability request");
    assert!(
        handle
            .effective_capabilities
            .iter()
            .any(|capability| capability == "sdk.capability.shared_instance_rpc_auth"),
        "shared-instance auth capability should be present in effective capability set"
    );
}
