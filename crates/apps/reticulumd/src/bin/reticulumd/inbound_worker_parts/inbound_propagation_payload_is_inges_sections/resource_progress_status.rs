use rns_transport::resource::ResourceProgress;

#[test]
fn inbound_resource_progress_status_preserves_bytes_and_parts() {
    let progress = ResourceProgress {
        received_bytes: 256,
        total_bytes: 1024,
        received_parts: 2,
        total_parts: 8,
    };

    assert_eq!(
        super::resource_progress_status("resource-hash", "link-id", &progress),
        "resource progress hash=resource-hash link=link-id bytes=256/1024 parts=2/8"
    );
}
