#[test]
fn parses_python_default_config() {
    let ini =
        std::fs::read_to_string("tests/fixtures/python/reticulum/config_default.ini").unwrap();
    let cfg = reticulum::config::Config::from_ini(&ini).unwrap();
    assert!(!cfg.interfaces.is_empty());
}
