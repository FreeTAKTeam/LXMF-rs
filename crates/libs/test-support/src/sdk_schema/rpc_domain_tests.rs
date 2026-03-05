use super::*;
use std::collections::BTreeSet;

#[test]
fn sdk_rpc_domain_schema_release_b_fixtures_are_validated() {
    let schemas = load_rpc_domain_schemas();
    let root = workspace_root();
    for path in fixture_paths("docs/fixtures/sdk-v2/rpc/release-b") {
        let relative = path
            .strip_prefix(&root)
            .map(|item| item.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());
        let json = read_json(&path);
        if relative.contains(".valid.") {
            assert_schema_valid(&schemas.release_b_methods, relative.as_str(), &json);
            continue;
        }
        if relative.contains(".invalid.") {
            assert_schema_invalid(&schemas.release_b_methods, relative.as_str(), &json);
            continue;
        }
        panic!("unexpected fixture naming, expected .valid. or .invalid. in {relative}");
    }
}

#[test]
fn sdk_rpc_domain_schema_release_c_fixtures_are_validated() {
    let schemas = load_rpc_domain_schemas();
    let root = workspace_root();
    for path in fixture_paths("docs/fixtures/sdk-v2/rpc/release-c") {
        let relative = path
            .strip_prefix(&root)
            .map(|item| item.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());
        let json = read_json(&path);
        if relative.contains(".valid.") {
            assert_schema_valid(&schemas.release_c_methods, relative.as_str(), &json);
            continue;
        }
        if relative.contains(".invalid.") {
            assert_schema_invalid(&schemas.release_c_methods, relative.as_str(), &json);
            continue;
        }
        panic!("unexpected fixture naming, expected .valid. or .invalid. in {relative}");
    }
}

#[test]
fn sdk_rpc_release_b_streaming_methods_have_paired_response_fixtures() {
    let expected_methods = [
        "sdk_attachment_upload_start_v2",
        "sdk_attachment_upload_chunk_v2",
        "sdk_attachment_upload_commit_v2",
        "sdk_attachment_download_chunk_v2",
    ];

    let mut names = BTreeSet::new();
    for path in fixture_paths("docs/fixtures/sdk-v2/rpc/release-b") {
        if let Some(file_name) = path.file_name().and_then(|item| item.to_str()) {
            names.insert(file_name.to_string());
        }
    }

    for method in expected_methods {
        let request_valid = format!("{method}.request.valid.json");
        let request_invalid = format!("{method}.request.invalid.json");
        let response_ok_valid = format!("{method}.response.ok.valid.json");
        let response_ok_invalid = format!("{method}.response.ok.invalid.json");
        let response_error_valid = format!("{method}.response.error.valid.json");

        assert!(
            names.contains(&request_valid),
            "missing fixture: {request_valid}"
        );
        assert!(
            names.contains(&request_invalid),
            "missing fixture: {request_invalid}"
        );
        assert!(
            names.contains(&response_ok_valid),
            "missing fixture: {response_ok_valid}"
        );
        assert!(
            names.contains(&response_ok_invalid),
            "missing fixture: {response_ok_invalid}"
        );
        assert!(
            names.contains(&response_error_valid),
            "missing fixture: {response_error_valid}"
        );
    }
}
