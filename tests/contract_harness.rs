use lxmf::message::{Payload, WireMessage};
use lxmf::reticulum::Adapter;
use lxmf::router::{OutboundStatus, Router};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    PythonToPython,
    PythonToRust,
    RustToPython,
    RustToRust,
}

impl Lane {
    fn all() -> [Self; 4] {
        [
            Self::PythonToPython,
            Self::PythonToRust,
            Self::RustToPython,
            Self::RustToRust,
        ]
    }

    fn id(self) -> &'static str {
        match self {
            Self::PythonToPython => "L1",
            Self::PythonToRust => "L2",
            Self::RustToPython => "L3",
            Self::RustToRust => "L4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContractState {
    Queued,
    Sending,
    Sent,
    Delivered,
    Rejected,
    Failed,
    Cancelled,
    Deferred,
}

impl ContractState {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "sending" => Some(Self::Sending),
            "sent" => Some(Self::Sent),
            "delivered" => Some(Self::Delivered),
            "rejected" => Some(Self::Rejected),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "deferred" => Some(Self::Deferred),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContractStatus {
    state: ContractState,
    progress: u8,
    detail: Option<String>,
}

#[derive(Debug, Clone)]
struct ContractMessageInput {
    destination: [u8; 16],
    source: [u8; 16],
    content: Vec<u8>,
    title: Option<Vec<u8>>,
}

trait ContractAdapter {
    fn configure_policy(&mut self, require_auth: bool, allow_destination: Option<[u8; 16]>);
    fn send(&mut self, message: ContractMessageInput) -> String;
    fn cancel(&mut self, handle: &str) -> bool;
    fn tick(&mut self, max_outbound: usize);
    fn status(&self, handle: &str) -> Option<ContractStatus>;
}

#[derive(Default)]
struct RustContractAdapter {
    router: Router,
    handle_to_id: BTreeMap<String, Vec<u8>>,
    statuses: BTreeMap<String, ContractStatus>,
}

impl RustContractAdapter {
    fn new() -> Self {
        Self {
            router: Router::with_adapter(Adapter::new()),
            ..Self::default()
        }
    }
}

impl ContractAdapter for RustContractAdapter {
    fn configure_policy(&mut self, require_auth: bool, allow_destination: Option<[u8; 16]>) {
        self.router.set_auth_required(require_auth);
        if let Some(destination) = allow_destination {
            self.router.allow_destination(destination);
        }
    }

    fn send(&mut self, message: ContractMessageInput) -> String {
        let payload = Payload::new(
            1_700_000_000.0,
            Some(message.content),
            message.title,
            None,
            None,
        );
        let wire = WireMessage::new(message.destination, message.source, payload);
        let message_id = wire.message_id().to_vec();
        let handle = hex::encode(&message_id);
        self.router.enqueue_outbound(wire);
        self.handle_to_id.insert(handle.clone(), message_id);
        self.statuses.insert(
            handle.clone(),
            ContractStatus {
                state: ContractState::Queued,
                progress: 0,
                detail: None,
            },
        );
        handle
    }

    fn cancel(&mut self, handle: &str) -> bool {
        let Some(message_id) = self.handle_to_id.get(handle) else {
            return false;
        };

        let cancelled = self.router.cancel_outbound(message_id);
        if cancelled {
            self.statuses.insert(
                handle.to_string(),
                ContractStatus {
                    state: ContractState::Cancelled,
                    progress: 0,
                    detail: None,
                },
            );
        }
        cancelled
    }

    fn tick(&mut self, max_outbound: usize) {
        let results = self
            .router
            .handle_outbound(max_outbound)
            .expect("outbound processing");
        self.router.jobs();

        for result in results {
            let handle = hex::encode(&result.message_id);
            let (state, detail) = match result.status {
                OutboundStatus::Sent => (ContractState::Sent, None),
                OutboundStatus::DeferredNoAdapter => {
                    (ContractState::Deferred, Some("no adapter".into()))
                }
                OutboundStatus::DeferredAdapterError => {
                    (ContractState::Deferred, Some("adapter error".into()))
                }
                OutboundStatus::RejectedAuth => {
                    (ContractState::Rejected, Some("auth rejected".into()))
                }
                OutboundStatus::Ignored => (ContractState::Failed, Some("ignored".into())),
            };

            let progress = self
                .router
                .outbound_progress(&result.message_id)
                .unwrap_or(0);
            self.statuses.insert(
                handle,
                ContractStatus {
                    state,
                    progress,
                    detail,
                },
            );
        }
    }

    fn status(&self, handle: &str) -> Option<ContractStatus> {
        self.statuses.get(handle).cloned()
    }
}

#[derive(Debug, Deserialize)]
struct ContractFixture {
    id: String,
    description: String,
    destination_hex: String,
    source_hex: String,
    content: String,
    title: Option<String>,
    require_auth: Option<bool>,
    allow_destination: Option<bool>,
    cancel_before_tick: Option<bool>,
    run_tick: Option<bool>,
    max_outbound: Option<usize>,
    expected_state_before_tick: Option<String>,
    expected_state_after_tick: Option<String>,
    expect_cancel_result: Option<bool>,
}

fn load_fixture(path: &Path) -> ContractFixture {
    let bytes = std::fs::read(path).expect("fixture read");
    serde_json::from_slice(&bytes).expect("valid contract fixture json")
}

fn decode_hash_16(hex_value: &str) -> [u8; 16] {
    let bytes = hex::decode(hex_value).expect("valid hex hash");
    bytes.try_into().expect("16-byte destination/source hash")
}

fn run_fixture(fixture: &ContractFixture, lane: Lane) {
    let mut adapter: Box<dyn ContractAdapter> = match lane {
        Lane::RustToRust => Box::new(RustContractAdapter::new()),
        _ => {
            eprintln!(
                "skipping {} on {} adapter lane (not implemented yet)",
                fixture.id,
                lane.id()
            );
            return;
        }
    };

    let destination = decode_hash_16(&fixture.destination_hex);
    adapter.configure_policy(
        fixture.require_auth.unwrap_or(false),
        if fixture.allow_destination.unwrap_or(false) {
            Some(destination)
        } else {
            None
        },
    );

    let handle = adapter.send(ContractMessageInput {
        destination,
        source: decode_hash_16(&fixture.source_hex),
        content: fixture.content.as_bytes().to_vec(),
        title: fixture.title.as_ref().map(|t| t.as_bytes().to_vec()),
    });

    assert!(
        !handle.is_empty(),
        "scenario {}: adapter must return a stable handle",
        fixture.id
    );

    if let Some(expected) = fixture.expected_state_before_tick.as_deref() {
        assert_state(
            fixture,
            &handle,
            adapter.status(&handle),
            expected,
            "before tick",
        );
    }

    if fixture.cancel_before_tick.unwrap_or(false) {
        let cancelled = adapter.cancel(&handle);
        if let Some(expected) = fixture.expect_cancel_result {
            assert_eq!(
                cancelled, expected,
                "scenario {}: unexpected cancel() result",
                fixture.id
            );
        }
    }

    if fixture.run_tick.unwrap_or(false) {
        adapter.tick(fixture.max_outbound.unwrap_or(1));
    }

    if let Some(expected) = fixture.expected_state_after_tick.as_deref() {
        assert_state(
            fixture,
            &handle,
            adapter.status(&handle),
            expected,
            "after scenario actions",
        );
    }
}

fn assert_state(
    fixture: &ContractFixture,
    handle: &str,
    actual: Option<ContractStatus>,
    expected_raw: &str,
    phase: &str,
) {
    let expected = ContractState::parse(expected_raw).expect("known contract state in fixture");
    let actual = actual.unwrap_or_else(|| {
        panic!(
            "scenario {} [{}]: missing status for handle {}",
            fixture.id, phase, handle
        )
    });
    assert_eq!(
        actual.state, expected,
        "scenario {} [{}] {}: {}",
        fixture.id, phase, fixture.description, expected_raw
    );
}

#[test]
fn contract_lane_matrix_scaffold_lists_all_lanes() {
    let lanes = Lane::all();
    assert_eq!(lanes.len(), 4);
    assert_eq!(lanes[0].id(), "L1");
    assert_eq!(lanes[1].id(), "L2");
    assert_eq!(lanes[2].id(), "L3");
    assert_eq!(lanes[3].id(), "L4");
}

#[test]
fn contract_scenarios_c01_to_c03_on_rust_lane() {
    let fixtures = [
        "tests/fixtures/contract/C01_direct_send_success.json",
        "tests/fixtures/contract/C02_queue_then_tick.json",
        "tests/fixtures/contract/C03_cancel_queued_message.json",
    ];

    for fixture_path in fixtures {
        let fixture = load_fixture(Path::new(fixture_path));
        run_fixture(&fixture, Lane::RustToRust);
    }
}
