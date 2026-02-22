use super::*;

#[test]
fn sdk_cookbook_config_fixtures_validate_against_schema() {
    let schemas = load_schemas();
    let root = workspace_root();
    let fixtures = fixture_paths("docs/fixtures/sdk-v2/cookbook");
    assert!(!fixtures.is_empty(), "expected cookbook fixtures under docs/fixtures/sdk-v2/cookbook");

    for path in fixtures {
        let relative = path
            .strip_prefix(&root)
            .expect("fixture path should be under workspace root")
            .to_string_lossy()
            .replace('\\', "/");
        let json = read_json(&path);
        if relative.ends_with(".valid.json") {
            assert_schema_valid(&schemas.config, relative.as_str(), &json);
        } else if relative.ends_with(".invalid.json") {
            assert_schema_invalid(&schemas.config, relative.as_str(), &json);
        } else {
            panic!(
                "cookbook fixture naming must end with .valid.json or .invalid.json: {relative}"
            );
        }
    }
}
