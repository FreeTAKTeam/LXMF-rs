#[test]
fn udp_frame_roundtrip() {
    let frame = std::fs::read("tests/fixtures/python/reticulum/iface_frames.bin").unwrap();
    let decoded = reticulum::iface::udp::decode_frame(&frame).unwrap();
    let encoded = reticulum::iface::udp::encode_frame(&decoded).unwrap();
    assert_eq!(frame, encoded);
}
