use super::*;

#[tokio::test]
async fn disabled_transport_returns_destination_proofs_across_local_client_boundary_only() {
    for source_local in [false, true] {
        for proof_local in [false, true] {
            for explicit in [false, true] {
                let transport = Transport::new(TransportConfig::default());
                let (mut source_channel, source_iface, ingress_iface) = {
                    let manager = transport.iface_manager();
                    let mut manager = manager.lock().await;
                    let source_channel = manager.new_channel(8);
                    let source_parent = *source_channel.address();
                    let source_iface = if source_local {
                        assert!(manager.set_shared_instance(source_parent, true));
                        manager
                            .register_virtual_iface(source_parent, IfaceRole::Unicast)
                            .expect("local source")
                    } else {
                        source_parent
                    };
                    let ingress = manager.new_channel(8);
                    let ingress_parent = *ingress.address();
                    let ingress_iface = if proof_local {
                        assert!(manager.set_shared_instance(ingress_parent, true));
                        manager
                            .register_virtual_iface(ingress_parent, IfaceRole::Unicast)
                            .expect("local proof sender")
                    } else {
                        ingress_parent
                    };
                    // Keep the unrelated receiver alive to avoid making dispatch depend on it.
                    (source_channel, source_iface, (ingress_iface, ingress))
                };
                let (ingress_iface, _ingress_channel) = ingress_iface;
                let data = Packet {
                    destination: AddressHash::new_from_slice(&[42; 16]),
                    data: PacketDataBuffer::new_from_slice(b"original destination data"),
                    ..Packet::default()
                };
                let hash = data.hash();
                handle_data(&data, source_iface, transport.get_handler().lock().await).await;
                let mut proof_data = Vec::new();
                if explicit {
                    proof_data.extend_from_slice(hash.as_slice());
                }
                proof_data.extend_from_slice(&[0; ed25519_dalek::SIGNATURE_LENGTH]);
                let proof = Packet {
                    header: Header {
                        packet_type: PacketType::Proof,
                        destination_type: DestinationType::Single,
                        ..Header::default()
                    },
                    destination: AddressHash::new_from_hash(&hash),
                    data: PacketDataBuffer::new_from_slice(&proof_data),
                    ..Packet::default()
                };
                handle_proof(proof.clone(), transport.get_handler(), ingress_iface).await;
                // Never echo a proof back onto its own ingress interface.
                handle_proof(proof.clone(), transport.get_handler(), source_iface).await;
                let output = source_channel.tx_channel.try_recv();
                if source_local || proof_local {
                    let output = output
                        .expect("disabled transport must return a local-client destination proof");
                    assert!(
                        matches!(output.tx_type, TxMessageType::Direct(iface) if iface == source_iface)
                    );
                    assert_eq!(
                        output.packet.to_bytes().expect("forwarded proof"),
                        proof.to_bytes().expect("original proof")
                    );
                } else {
                    assert!(
                        matches!(output, Err(tokio::sync::mpsc::error::TryRecvError::Empty)),
                        "disabled ordinary transit remains blocked"
                    );
                }
                assert!(
                    matches!(
                        source_channel.tx_channel.try_recv(),
                        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
                    ),
                    "at most one proof is emitted"
                );
                transport.cancel.cancel();
            }
        }
    }
}

#[test]
#[ignore = "requires pinned Python Reticulum checkout at RETICULUM_PY_REPO"]
fn pinned_python_disabled_transport_local_proof_forwarding_policy() {
    let repo = std::env::var("RETICULUM_PY_REPO").expect("pinned Reticulum checkout");
    let revision = std::process::Command::new("git")
        .args(["-C", &repo, "rev-parse", "HEAD"])
        .output()
        .expect("reference revision");
    assert!(revision.status.success());
    assert_eq!(
        String::from_utf8_lossy(&revision.stdout).trim(),
        "99de23c040d507e3fefca19e87b182302902725d"
    );
    // Execute the actual frozen reverse-proof block in isolation. This proves its
    // policy and output, not socket ingress, receipt verification or table creation.
    let script = r#"
import ast, pathlib, struct, threading
from types import SimpleNamespace
source = pathlib.Path(__import__('os').environ['RETICULUM_PY_REPO'], 'RNS/Transport.py').read_text()
tree = ast.parse(source)
blocks = [node for node in ast.walk(tree) if isinstance(node, ast.If)
          and ast.unparse(node.test) == '(RNS.Reticulum.transport_enabled() or from_local_client or proof_for_local_client) and packet.destination_hash in Transport.reverse_table']
assert len(blocks) == 1
code = compile(ast.fix_missing_locations(ast.Module(body=blocks, type_ignores=[])), '<frozen reverse-proof block>', 'exec')
for source_local in (False, True):
    for proof_local in (False, True):
        sent = []
        transport = SimpleNamespace(reverse_table={b'dst': ['source', 'ingress', 0]}, reverse_table_lock=threading.Lock(), local_hops_delta=0,
            transmit=lambda iface, raw: sent.append((iface, raw)))
        packet = SimpleNamespace(destination_hash=b'dst', receiving_interface='ingress', raw=b'\x01\x00proof', hops=0)
        rns = SimpleNamespace(Reticulum=SimpleNamespace(transport_enabled=lambda: False), log=lambda *args: None, sl=lambda level: False, LOG_EXTREME=0, LOG_DEBUG=0)
        exec(code, dict(RNS=rns, Transport=transport, packet=packet, from_local_client=proof_local,
             proof_for_local_client=source_local, IDX_RT_RCVD_IF=0, IDX_RT_OUTB_IF=1, struct=struct))
        assert sent == ([('source', packet.raw)] if source_local or proof_local else [])
print('local-proof-policy=0,1,1,1')
"#;
    let output = std::process::Command::new(
        std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".into()),
    )
    .args(["-c", script])
    .output()
    .expect("frozen proof policy");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "local-proof-policy=0,1,1,1");
}
