#[test]
#[ignore = "requires exact RNS 1.5.5 checkout and reference Python"]
fn target_python_autoconnect_qualification_exclusions_and_names_match_native_planner() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let repo = std::env::var("RETICULUM_PY_REPO").expect("explicit target reference");
    let revision = Command::new("git")
        .args(["-C", &repo, "rev-parse", "HEAD"])
        .output()
        .expect("reference revision");
    assert!(revision.status.success());
    assert_eq!(
        String::from_utf8(revision.stdout).expect("revision text").trim(),
        "7f2b3b9b524c9386316379af1313b43a5e4f7a5d"
    );
    let mut cases = Vec::new();
    let mut native = Vec::new();
    for implementation in [None, Some("RNS"), Some("reticulumd")] {
        for version in [
            None,
            Some(""),
            Some("invalid"),
            Some("1"),
            Some("1.5"),
            Some("1.5.1"),
            Some("1.5.2"),
            Some("1.5.2rc1"),
            Some(" 1.5.2.dev "),
            Some("2"),
            Some("1.4.99999999999999999999999999"),
            Some("99999999999999999999999999"),
            Some("1.5.99999999999999999999999999"),
        ] {
            for allow_unverified in [false, true] {
                cases.push(serde_json::json!({"impl_name": implementation,
                    "version": version, "override": allow_unverified}));
                let mut info = discovered("relay", "relay.example", 4242);
                info.impl_name = implementation.map(str::to_owned);
                info.version = version.map(str::to_owned);
                info.transport = false;
                native.push(
                    plan_autoconnect_with_policy(
                        &info,
                        &[],
                        AutoconnectInterfacePolicy { allow_unverified, ..Default::default() },
                    )
                    .is_some(),
                );
            }
        }
    }
    assert_eq!(cases.len(), 78);
    let endpoints = [
        "127.0.0.1",
        "0.0.0.0",
        "relay.ONION",
        "200::1",
        "3ff::1",
        "127.0.0.2",
        "::1",
        "400::1",
        "relay.example",
    ];
    let native_endpoints = endpoints
        .iter()
        .map(|host| plan_autoconnect(&discovered("relay", host, 4242), &[]).is_some())
        .collect::<Vec<_>>();
    let names = ["relay", "relay (2)"].map(|name| RuntimeInterfaceState {
        id: name.into(),
        autoconnect_hash: None,
        autoconnect_source: None,
        target_host: None,
        target_port: None,
        i2p_b32: None,
        bootstrap_only: false,
        online: false,
        down_since: None,
    });
    let plans = InterfaceDiscoveryLifecycle::default().initial_autoconnect(
        &[discovered("relay", "one.example", 4242), discovered("relay", "two.example", 4242)],
        &names,
        2,
    );
    let native_names = plans.iter().map(|plan| &plan.name).collect::<Vec<_>>();
    // Controlled primary-source methods, not a Python network/process trace.
    let script = r#"
import json, sys
from types import SimpleNamespace
sys.path.insert(0, sys.argv[1])
import RNS
from RNS.Discovery import InterfaceDiscovery, is_ip_address, is_invalid_ip_address, is_onion_address, is_ygg_ipv6
data = json.load(sys.stdin)
owner = InterfaceDiscovery.__new__(InterfaceDiscovery)
rows = []
for row in data['cases']:
    RNS.Reticulum.should_autoconnect_unverified_implementations = staticmethod(lambda: row['override'])
    rows.append(owner.autoconnect_qualified(row))
endpoints = [not (is_onion_address(host) or is_ygg_ipv6(host) or
    (is_ip_address(host) and is_invalid_ip_address(host))) for host in data['endpoints']]
RNS.Transport.interfaces = [SimpleNamespace(name=name) for name in ['relay', 'relay (2)']]
names = []
for _ in range(2):
    name = owner.autoconnect_interface_name('relay')
    names.append(name)
    RNS.Transport.interfaces.append(SimpleNamespace(name=name))
print(json.dumps({'rows': rows, 'endpoints': endpoints, 'names': names}))
"#;
    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".into());
    let mut child = Command::new(python)
        .args(["-c", script, &repo])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("target Python probe");
    child
        .stdin
        .take()
        .expect("probe input")
        .write_all(
            &serde_json::to_vec(&serde_json::json!({"cases": cases, "endpoints": endpoints}))
                .expect("probe cases"),
        )
        .expect("write probe cases");
    let result = child.wait_with_output().expect("target Python output");
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let reference: serde_json::Value =
        serde_json::from_slice(&result.stdout).expect("reference rows");
    assert_eq!(
        reference,
        serde_json::json!({"rows": native, "endpoints": native_endpoints, "names": native_names})
    );
}
