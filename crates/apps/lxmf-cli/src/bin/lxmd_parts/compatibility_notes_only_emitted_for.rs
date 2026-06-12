#[cfg(test)]
mod tests {
    use super::config::{
        apply_config_file, is_single_toml_config, load_effective_args, prepare_lxmd_paths,
    };
    use super::config_python::{parse_python_lxmd_config, parse_python_reticulum_interfaces};
    use super::python_compat::compatibility_notes;
    use super::{requires_supervised_launch, EffectiveArgs};
    use clap::Parser;
    use serde_json::json;
    use std::fs;

    #[test]
    fn compatibility_notes_only_emitted_for_used_flags() {
        let args = super::Args::parse_from(["lxmd", "--propagation-node", "--service"]);
        let effective = EffectiveArgs {
            profile: "default".into(),
            rpc: super::DEFAULT_RPC_ADDR.into(),
            rnsconfig: None,
            propagation_node: true,
            on_inbound: None,
            quiet: false,
            service: true,
            display_name: None,
            db: None,
            identity: None,
            transport: None,
            reticulumd: None,
            messages_dir: None,
            config_dir: None,
            timeout_secs: 5.0,
            status: false,
            peers: false,
            sync: None,
            unpeer: None,
            remote: None,
            query_identity: None,
            python_compat: super::PythonCompatConfig::default(),
        };
        let notes = compatibility_notes(&args, &effective);
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn propagation_node_uses_supervised_launch() {
        let effective = EffectiveArgs {
            profile: "default".into(),
            rpc: super::DEFAULT_RPC_ADDR.into(),
            rnsconfig: None,
            propagation_node: true,
            on_inbound: None,
            quiet: false,
            service: false,
            display_name: None,
            db: None,
            identity: None,
            transport: None,
            reticulumd: None,
            messages_dir: None,
            config_dir: None,
            timeout_secs: 5.0,
            status: false,
            peers: false,
            sync: None,
            unpeer: None,
            remote: None,
            query_identity: None,
            python_compat: super::PythonCompatConfig::default(),
        };
        assert!(requires_supervised_launch(&effective));
    }

    #[test]
    fn rpc_frame_roundtrips() {
        let value = json!({
            "id": 1,
            "result": {
                "propagation": {
                    "enabled": true
                }
            }
        });
        let encoded = super::rpc_client::encode_rpc_frame(value.clone()).expect("encode");
        let decoded = super::rpc_client::decode_rpc_frame(&encoded).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn config_file_applies_lxmd_launcher_settings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("lxmd.toml");
        fs::write(
            &path,
            r#"[lxmd]
profile = "alpha"
rpc = "127.0.0.1:5555"
rnsconfig = "/tmp/reticulum.toml"
propagation_node = true
on_inbound = "echo hi"
display_name = "node-a"
db = "/tmp/reticulum.db"
identity = "/tmp/identity"
transport = "127.0.0.1:4242"
reticulumd = "/tmp/reticulumd"
"#,
        )
        .expect("write config");

        let mut effective = EffectiveArgs {
            profile: "default".into(),
            rpc: "127.0.0.1:4243".into(),
            rnsconfig: None,
            propagation_node: false,
            on_inbound: None,
            quiet: false,
            service: false,
            display_name: None,
            db: None,
            identity: None,
            transport: None,
            reticulumd: None,
            messages_dir: None,
            config_dir: None,
            timeout_secs: 5.0,
            status: false,
            peers: false,
            sync: None,
            unpeer: None,
            remote: None,
            query_identity: None,
            python_compat: super::PythonCompatConfig::default(),
        };
        apply_config_file(&mut effective, &path).expect("apply config");
        assert_eq!(effective.profile, "alpha");
        assert_eq!(effective.rpc, "127.0.0.1:5555");
        assert!(effective.propagation_node);
        assert_eq!(effective.on_inbound.as_deref(), Some("echo hi"));
        assert_eq!(effective.display_name.as_deref(), Some("node-a"));
        assert_eq!(effective.transport.as_deref(), Some("127.0.0.1:4242"));
    }

    #[test]
    fn single_toml_config_is_detected_and_loaded() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("lxmd.toml");
        fs::write(
            &path,
            r#"[node]
display_name = "Node A"

[rpc]
listen = "127.0.0.1:5555"

[transport]
listen = "0.0.0.0:37777"

[storage]
db = "./state/reticulum.db"
identity = "./state/identity"

[propagation]
enable = true
announce_at_start = true
announce_interval = 90
autopeer = true
autopeer_maxdepth = 7
retain_synced_on_node = true
auth_required = true

[lxmf]
on_inbound = "echo hi"

[[interfaces]]
type = "tcp_client"
enabled = true
name = "rmap.world"
host = "rmap.world"
port = 4242
"#,
        )
        .expect("write config");

        assert!(is_single_toml_config(&path).expect("detect single toml"));
        let args = super::Args::parse_from(["lxmd", "--config", path.to_str().expect("utf8 path")]);
        let effective = load_effective_args(&args).expect("effective args");
        assert_eq!(effective.rpc, "127.0.0.1:5555");
        assert_eq!(effective.transport.as_deref(), Some("0.0.0.0:37777"));
        assert_eq!(effective.display_name.as_deref(), Some("Node A"));
        assert!(effective.propagation_node);
        assert_eq!(effective.on_inbound.as_deref(), Some("echo hi"));
        assert!(effective.python_compat.auth_required);
        assert_eq!(effective.python_compat.autopeer_maxdepth, Some(7));
        assert!(effective.python_compat.retain_synced_on_node);
        assert_eq!(effective.db.as_deref(), Some(temp.path().join("state/reticulum.db").as_path()));
        assert_eq!(
            effective.identity.as_deref(),
            Some(temp.path().join("state/identity").as_path())
        );
        let generated = temp.path().join(super::GENERATED_RETICULUMD_CONFIG);
        let generated_contents =
            fs::read_to_string(&generated).expect("generated reticulum config");
        assert!(generated_contents.contains("host = \"rmap.world\""));
        assert!(generated_contents.contains("port = 4242"));
    }

    #[test]
    fn python_reticulum_interfaces_parse_tcp_server_and_client() {
        let interfaces = parse_python_reticulum_interfaces(
            r#"
[interfaces]
  [[Server]]
    type = TCPServerInterface
    enabled = yes
    listen_ip = 0.0.0.0
    listen_port = 4242

  [[Client]]
    type = TCPClientInterface
    enabled = yes
    target_host = rmap.world
    target_port = 4243
"#,
        );
        assert_eq!(interfaces.len(), 2);
        assert_eq!(interfaces[0].interface_type, "tcp_server");
        assert_eq!(interfaces[0].host.as_deref(), Some("0.0.0.0"));
        assert_eq!(interfaces[0].port, Some(4242));
        assert_eq!(interfaces[1].interface_type, "tcp_client");
        assert_eq!(interfaces[1].host.as_deref(), Some("rmap.world"));
        assert_eq!(interfaces[1].port, Some(4243));
    }

    #[test]
    fn python_config_generates_reticulumd_interfaces_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("lxmd");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        let config_path = config_dir.join("config");
        fs::write(
            &config_path,
            r#"
[propagation]
enable_node = yes
retain_synced_on_node = yes
auth_required = yes

[interfaces]
  [[Server]]
    type = TCPServerInterface
    enabled = yes
    listen_port = 4242
"#,
        )
        .expect("write config");

        let args =
            super::Args::parse_from(["lxmd", "--config", config_path.to_str().expect("utf8 path")]);
        let effective = load_effective_args(&args).expect("effective args");
        let generated = config_dir.join(super::GENERATED_RETICULUMD_CONFIG);
        let generated_contents = fs::read_to_string(&generated).expect("generated config");

        assert!(effective.python_compat.retain_synced_on_node);
        assert_eq!(effective.rnsconfig.as_deref(), Some(generated.as_path()));
        assert!(effective.python_compat.auth_required);
        assert!(generated_contents.contains("type = \"tcp_server\""));
        assert!(generated_contents.contains("host = \"0.0.0.0\""));
        assert!(generated_contents.contains("port = 4242"));
    }

    #[test]
    fn python_config_keeps_lxmf_peer_and_propagation_node_announce_intervals_separate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("lxmd");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        let config_path = config_dir.join("config");
        fs::write(
            &config_path,
            r#"
[lxmf]
announce_interval = 3

[propagation]
announce_interval = 2
"#,
        )
        .expect("write config");

        let args =
            super::Args::parse_from(["lxmd", "--config", config_path.to_str().expect("utf8 path")]);
        let effective = load_effective_args(&args).expect("effective args");

        assert_eq!(effective.python_compat.peer_announce_interval_min, Some(3));
        assert_eq!(effective.python_compat.node_announce_interval_min, Some(2));
    }

    #[test]
    fn python_config_sections_are_parsed() {
        let sections = parse_python_lxmd_config(
            r#"
[propagation]
enable_node = yes

[lxmf]
display_name = Anonymous Peer
on_inbound = rm
"#,
        );
        assert_eq!(sections["propagation"]["enable_node"], "yes");
        assert_eq!(sections["lxmf"]["display_name"], "Anonymous Peer");
        assert_eq!(sections["lxmf"]["on_inbound"], "rm");
    }

    #[test]
    fn prepare_lxmd_paths_creates_python_style_layout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("lxmd");
        let paths = prepare_lxmd_paths(Some(&config_dir)).expect("paths");
        assert!(paths.config_file.exists());
        assert!(paths.messages_dir.exists());
        assert_eq!(paths.identity_file, config_dir.join("identity"));
    }
}
