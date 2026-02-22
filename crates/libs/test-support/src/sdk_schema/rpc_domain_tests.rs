use super::*;

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
