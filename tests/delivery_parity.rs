use serde::Deserialize;

use lxmf::message::{decide_delivery, MessageMethod, TransportMethod};

#[derive(Deserialize)]
struct DeliveryCase {
    desired_method: u8,
    destination_plain: bool,
    content_size: usize,
    expected_method: u8,
    expected_representation: u8,
}

#[test]
fn delivery_selection_matches_python_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/delivery_matrix.msgpack").unwrap();
    let cases: Vec<DeliveryCase> = rmp_serde::from_slice(&bytes).unwrap();

    for case in cases {
        let desired = TransportMethod::try_from(case.desired_method).expect("valid desired method");
        let decision = decide_delivery(desired, case.destination_plain, case.content_size)
            .expect("delivery decision");

        assert_eq!(decision.method.as_u8(), case.expected_method);
        assert_eq!(
            decision.representation.as_u8(),
            case.expected_representation
        );

        let expected_repr =
            MessageMethod::try_from(case.expected_representation).expect("valid representation");
        assert_eq!(decision.representation, expected_repr);
    }
}
