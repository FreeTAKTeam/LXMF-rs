use lxmf::cli::profile::{
    init_profile, list_profiles, load_profile_settings, load_reticulum_config, profile_paths,
    remove_interface, save_reticulum_config, select_profile, selected_profile_name,
    set_interface_enabled, upsert_interface, InterfaceEntry,
};
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn profile_init_and_select_roundtrip() {
    let _guard = env_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    let created = init_profile("alpha", true, Some("127.0.0.1:9999".into())).unwrap();
    assert_eq!(created.name, "alpha");
    assert!(created.managed);

    select_profile("alpha").unwrap();
    let selected = selected_profile_name().unwrap();
    assert_eq!(selected.as_deref(), Some("alpha"));

    let listed = list_profiles().unwrap();
    assert_eq!(listed, vec!["alpha".to_string()]);

    let loaded = load_profile_settings("alpha").unwrap();
    assert_eq!(loaded.rpc, "127.0.0.1:9999");

    let paths = profile_paths("alpha").unwrap();
    assert!(paths.profile_toml.exists());
    assert!(paths.reticulum_toml.exists());

    std::env::remove_var("LXMF_CONFIG_ROOT");
}

#[test]
fn reticulum_interface_mutations_persist() {
    let _guard = env_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("LXMF_CONFIG_ROOT", temp.path());

    init_profile("beta", false, None).unwrap();

    let mut config = load_reticulum_config("beta").unwrap();
    upsert_interface(
        &mut config,
        InterfaceEntry {
            name: "uplink".into(),
            kind: "tcp_client".into(),
            enabled: true,
            host: Some("127.0.0.1".into()),
            port: Some(4242),
        },
    );
    save_reticulum_config("beta", &config).unwrap();

    let mut loaded = load_reticulum_config("beta").unwrap();
    assert_eq!(loaded.interfaces.len(), 1);
    assert!(set_interface_enabled(&mut loaded, "uplink", false));
    assert!(!loaded.interfaces[0].enabled);
    assert!(remove_interface(&mut loaded, "uplink"));
    assert!(loaded.interfaces.is_empty());

    std::env::remove_var("LXMF_CONFIG_ROOT");
}
