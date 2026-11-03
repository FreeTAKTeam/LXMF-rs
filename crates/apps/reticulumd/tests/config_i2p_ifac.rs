use reticulum_daemon::config::DaemonConfig;

fn parse(settings: &str) -> reticulum_daemon::config::InterfaceConfig {
    DaemonConfig::from_toml(&format!(
        "interfaces=[{{type='I2PInterface',enabled=true,connectable=true,{settings}}}]"
    ))
    .expect("valid I2P configuration")
    .interfaces
    .remove(0)
}

#[test]
fn i2p_ifac_aliases_preserve_whitespace_literally() {
    for (network, key) in [(" ", ""), ("", " "), (" \t ", " key ")] {
        let iface = parse(&format!("ifac_netname='{network}',ifac_netkey='{key}'"));
        assert_eq!(
            iface.ifac_network_name().map(String::as_str),
            (!network.is_empty()).then_some(network)
        );
        assert_eq!(iface.ifac_passphrase().map(String::as_str), (!key.is_empty()).then_some(key));
    }
}

#[test]
fn i2p_ifac_empty_canonical_credentials_fall_back_to_legacy_aliases() {
    let iface = parse("network_name='',networkname='',pass_phrase='',passphrase='',ifac_netname='field-network',ifac_netkey='field-key'");
    assert_eq!(iface.ifac_network_name().map(String::as_str), Some("field-network"));
    assert_eq!(iface.ifac_passphrase().map(String::as_str), Some("field-key"));
}

#[test]
fn i2p_ifac_populated_canonical_credentials_keep_precedence() {
    let iface = parse("network_name='network',networkname='alternate',pass_phrase='key',passphrase='alternate-key',ifac_netname='legacy-network',ifac_netkey='legacy-key'");
    assert_eq!(iface.ifac_network_name().map(String::as_str), Some("network"));
    assert_eq!(iface.ifac_passphrase().map(String::as_str), Some("key"));
}

#[test]
fn i2p_ifac_empty_legacy_credentials_keep_plaintext_default() {
    let iface = parse("ifac_netname='',ifac_netkey=''");
    assert_eq!(iface.ifac_network_name(), None);
    assert_eq!(iface.ifac_passphrase(), None);
}
