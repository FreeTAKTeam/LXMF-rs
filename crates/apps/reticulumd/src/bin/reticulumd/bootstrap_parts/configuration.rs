use super::*;
use reticulum_daemon::config::ReticulumRuntimePolicy;
use std::path::Path;

pub(super) struct ParsedRuntimePolicy {
    policy: ReticulumRuntimePolicy,
    allow_unverified: bool,
    announce_defaults: (u64, u64, u64),
}

impl Default for ParsedRuntimePolicy {
    fn default() -> Self {
        Self {
            policy: ReticulumRuntimePolicy::default(),
            allow_unverified: false,
            announce_defaults: (3600, 5, 0),
        }
    }
}

impl ParsedRuntimePolicy {
    pub(super) fn announce_defaults(&self) -> (u64, u64, u64) {
        self.announce_defaults
    }
    pub(super) fn into_parts(self) -> (ReticulumRuntimePolicy, bool) {
        (self.policy, self.allow_unverified)
    }
}

pub(super) fn load(
    path: Option<&Path>,
) -> (Option<DaemonConfig>, ParsedRuntimePolicy, Vec<InterfaceRecord>) {
    match path {
        Some(path) => load_with(path, |path| std::fs::read_to_string(path)),
        None => (None, ParsedRuntimePolicy::default(), Vec::new()),
    }
}

fn load_with(
    path: &Path,
    read: impl FnOnce(&Path) -> std::io::Result<String>,
) -> (Option<DaemonConfig>, ParsedRuntimePolicy, Vec<InterfaceRecord>) {
    let input = match read(path) {
        Ok(input) => input,
        Err(error) => {
            log::error!("[daemon] failed to read config {}: {error}", path.display());
            return (None, ParsedRuntimePolicy::default(), Vec::new());
        }
    };
    let (daemon, rejected) = match DaemonConfig::from_toml(&input) {
        Ok(config) => (Some(config), Vec::new()),
        Err(error) => {
            log::error!("[daemon] failed to parse config {}: {error}", path.display());
            (None, rejected_ifac_interface_records(&input))
        }
    };
    let policy = match ReticulumRuntimePolicy::from_toml(&input) {
        Ok(policy) => policy,
        Err(error) => {
            log::error!(
                "[daemon] failed to parse Reticulum runtime policy {}: {error}",
                path.display()
            );
            ReticulumRuntimePolicy::default()
        }
    };
    // The additive option was ignored by both established public views. A bad
    // value disables only this override, preserving their existing fallbacks.
    #[derive(Default, serde::Deserialize)]
    struct ExtraPolicy {
        #[serde(default)]
        autoconnect_unverified_implementations: bool,
    }
    #[derive(serde::Deserialize)]
    struct ExtraDocument {
        #[serde(default)]
        reticulum: ExtraPolicy,
    }
    let allow_unverified = match toml::from_str::<ExtraDocument>(&input) {
        Ok(document) => document.reticulum.autoconnect_unverified_implementations,
        Err(error) => {
            log::warn!("[daemon] invalid discovery override in {}: {error}; using verified implementations only", path.display());
            false
        }
    };
    let mut announce_defaults = [3600, 5, 0];
    if let Ok(document) = input.parse::<toml::Value>() {
        for ((name, fallback), output) in
            [("default_ar_target", 3600), ("default_ar_grace", 5), ("default_ar_penalty", 0)]
                .into_iter()
                .zip(&mut announce_defaults)
        {
            if let Some(value) = document.get("reticulum").and_then(|table| table.get(name)) {
                match value.as_integer().and_then(|value| u64::try_from(value).ok()) {
                    Some(0) => *output = fallback,
                    Some(value) => *output = value,
                    None => log::warn!("[daemon] invalid discovery announce default option={name}; using carrier default"),
                }
            }
        }
    }
    (
        daemon,
        ParsedRuntimePolicy {
            policy,
            allow_unverified,
            announce_defaults: (announce_defaults[0], announce_defaults[1], announce_defaults[2]),
        },
        rejected,
    )
}

fn rejected_ifac_interface_records(source: &str) -> Vec<InterfaceRecord> {
    let Ok(document) = source.parse::<toml::Value>() else { return Vec::new() };
    let Some(interfaces) = document.get("interfaces") else { return Vec::new() };
    let rows: Vec<(Option<&str>, &toml::Value)> = match interfaces {
        toml::Value::Array(items) => items.iter().map(|item| (None, item)).collect(),
        toml::Value::Table(items) => {
            items.iter().map(|(name, item)| (Some(name.as_str()), item)).collect()
        }
        _ => return Vec::new(),
    };
    rows.into_iter()
        .enumerate()
        .filter_map(|(index, (section_name, value))| {
            let table = value.as_table()?;
            let ifac_size = table.get("ifac_size")?;
            let valid_ifac_size = ifac_size
                .as_integer()
                .and_then(|value| u64::try_from(value).ok())
                .is_some_and(|bits| bits / 8 <= 64);
            let credential_present = ["network_name", "networkname", "passphrase", "pass_phrase"]
                .iter()
                .filter_map(|key| table.get(*key).and_then(toml::Value::as_str))
                .any(|credential| !credential.is_empty());
            let name = table.get("name").and_then(toml::Value::as_str).or(section_name)
                .map(str::to_owned).or_else(|| Some(format!("interfaces[{index}]")));
            let kind = table.get("type").and_then(toml::Value::as_str).or(section_name)
                .unwrap_or("unknown").to_owned();
            let startup_error = if valid_ifac_size && credential_present {
                "daemon configuration rejected before interface startup; inspect daemon log"
            } else {
                "IFAC configuration rejected: ifac_size must floor to 1..=64 bytes (or be below the one-byte minimum to use the carrier default) and requires a non-empty network_name or passphrase"
            };
            let settings = json!({"_runtime": {
                "startup_status": "failed",
                "startup_error": startup_error
            }});
            Some(InterfaceRecord {
                kind,
                enabled: table.get("enabled").and_then(toml::Value::as_bool)
                    .or_else(|| table.get("interface_enabled").and_then(toml::Value::as_bool))
                    .unwrap_or(false),
                host: table.get("host").and_then(toml::Value::as_str).map(str::to_owned),
                port: table.get("port").and_then(toml::Value::as_integer).and_then(|port| u16::try_from(port).ok()),
                name,
                settings: Some(settings),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured(override_value: &str) -> String {
        format!("[reticulum]\nshare_instance=false\nenable_transport=true\ndiscover_interfaces=true\nautoconnect_discovered_interfaces=3\nenable_interface_management=false\n{override_value}\n[[interfaces]]\ntype='tcp_client'\nname='snapshot-first'\nhost='127.0.0.2'\nport=4242\nenabled=true\n")
    }

    #[test]
    fn discovery_override_is_independent_of_established_public_views() {
        for (setting, allowed) in [
            ("", false),
            ("autoconnect_unverified_implementations=true", true),
            ("autoconnect_unverified_implementations=false", false),
            ("autoconnect_unverified_implementations='invalid'", false),
        ] {
            let input = configured(setting);
            let expected = ReticulumRuntimePolicy::from_toml(&input).expect("old public view");
            let (daemon, parsed, rejected) = load_with(Path::new("config"), |_| Ok(input));
            assert_eq!(parsed.allow_unverified, allowed);
            assert_eq!(parsed.policy, expected);
            assert_eq!(parsed.policy.max_autoconnected_interfaces, 3);
            assert!(!parsed.policy.interface_management_enabled);
            assert!(daemon
                .expect("existing daemon view remains valid")
                .reticulum_transport_enabled());
            assert!(rejected.is_empty());
        }
    }

    #[test]
    fn discovery_announce_defaults_preserve_target_zero_and_invalid_fallbacks() {
        for (setting, expected) in [
            ("", (3600, 5, 0)),
            ("default_ar_target=120\ndefault_ar_grace=3\ndefault_ar_penalty=8", (120, 3, 8)),
            ("default_ar_target=0\ndefault_ar_grace=0\ndefault_ar_penalty=0", (3600, 5, 0)),
            ("default_ar_target=-2\ndefault_ar_grace='invalid'", (3600, 5, 0)),
        ] {
            let input =
                configured(&format!("autoconnect_unverified_implementations=true\n{setting}"));
            let (daemon, parsed, _) = load_with(Path::new("config"), |_| Ok(input));
            assert!(daemon.is_some());
            assert!(parsed.allow_unverified, "independent additive option");
            assert_eq!(parsed.policy.max_autoconnected_interfaces, 3);
            assert_eq!(parsed.announce_defaults(), expected);
        }
    }

    #[test]
    fn invalid_interface_retains_valid_runtime_and_sanitized_rejection_view() {
        let input = configured("autoconnect_unverified_implementations=true") + "ifac_size=16\n";
        assert!(DaemonConfig::from_toml(&input).is_err());
        let expected = ReticulumRuntimePolicy::from_toml(&input)
            .expect("valid runtime despite interface failure");
        let (daemon, parsed, rejected) = load_with(Path::new("config"), |_| Ok(input));
        assert!(daemon.is_none());
        assert_eq!(parsed.policy, expected);
        assert!(parsed.allow_unverified);
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].name.as_deref(), Some("snapshot-first"));
        assert_eq!(
            rejected[0].settings.as_ref().expect("diagnostic")["_runtime"]["startup_status"],
            "failed"
        );
    }

    #[test]
    fn one_read_keeps_all_views_on_the_same_snapshot_when_file_is_replaced() {
        let temp = tempfile::tempdir().expect("configuration directory");
        let path = temp.path().join("reticulum.toml");
        let first = configured("autoconnect_unverified_implementations=true");
        let replacement = "[reticulum]\nshare_instance=false\nenable_transport=false\nautoconnect_discovered_interfaces=9\n";
        std::fs::write(&path, &first).expect("first snapshot");
        let reads = std::cell::Cell::new(0);
        let (daemon, parsed, rejected) = load_with(&path, |path| {
            reads.set(reads.get() + 1);
            let snapshot = std::fs::read_to_string(path)?;
            std::fs::write(path, replacement)?;
            Ok(snapshot)
        });
        assert_eq!(reads.get(), 1);
        assert_eq!(std::fs::read_to_string(&path).expect("replacement"), replacement);
        let daemon = daemon.expect("first daemon view");
        assert!(daemon.reticulum_transport_enabled());
        assert_eq!(daemon.interfaces[0].name.as_deref(), Some("snapshot-first"));
        assert_eq!(parsed.policy.max_autoconnected_interfaces, 3);
        assert!(parsed.allow_unverified);
        assert!(rejected.is_empty());
    }

    #[test]
    fn absent_unreadable_and_invalid_established_runtime_keep_their_defaults() {
        let (daemon, parsed, rejected) = load(None);
        assert!(daemon.is_none());
        assert_eq!(parsed.policy, ReticulumRuntimePolicy::default());
        assert!(!parsed.allow_unverified);
        assert!(rejected.is_empty());
        let (daemon, parsed, rejected) = load_with(Path::new("unreadable"), |_| {
            Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "fixture"))
        });
        assert!(daemon.is_none());
        assert_eq!(parsed.policy, ReticulumRuntimePolicy::default());
        assert!(!parsed.allow_unverified);
        assert!(rejected.is_empty());
        let input = "[reticulum]\ninterface_discovery_sources=['invalid']\n";
        assert!(DaemonConfig::from_toml(input).is_err());
        assert!(ReticulumRuntimePolicy::from_toml(input).is_err());
        let (daemon, parsed, _) = load_with(Path::new("invalid"), |_| Ok(input.into()));
        assert!(daemon.is_none());
        assert_eq!(parsed.policy, ReticulumRuntimePolicy::default());
        assert!(!parsed.allow_unverified);
    }
}
