use rns_transport::{buffer::OutputBuffer, iface::hdlc::Hdlc};

#[test]
fn rns_1_5_4_hdlc_frames_match_pinned_python_vectors() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../test-support/fixtures/rns_1_5_4_hdlc.json"))
            .expect("reference fixture");
    assert_eq!(fixture["reference"], "99de23c040d507e3fefca19e87b182302902725d");
    for case in fixture["vectors"].as_array().expect("vectors") {
        let payload = hex::decode(case["payload"].as_str().expect("payload")).expect("payload hex");
        let expected = hex::decode(case["frame"].as_str().expect("frame")).expect("frame hex");
        assert_eq!(Hdlc::frame(&payload).expect("frame"), expected, "{}", case["name"]);
        let mut bytes = vec![0_u8; expected.len()];
        let mut buffer = OutputBuffer::new(&mut bytes);
        Hdlc::encode(&payload, &mut buffer).expect("existing stream encoder");
        assert_eq!(buffer.as_slice(), expected, "{}", case["name"]);
        let mut decoded = vec![0_u8; payload.len()];
        let mut buffer = OutputBuffer::new(&mut decoded);
        Hdlc::decode(&expected, &mut buffer).expect("decode");
        assert_eq!(buffer.as_slice(), payload, "{}", case["name"]);
    }
}
