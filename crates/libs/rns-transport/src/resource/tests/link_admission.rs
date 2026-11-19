//! Issue #766: per-Link inbound Resource admission for application protocols.
use super::*;

fn advertisement(seed: u8, data_size: u64) -> ResourceAdvertisement {
    let mut advertisement = advertisement_with(data_size, data_size, 1);
    advertisement.hash = Hash::new_from_slice(&[seed; 32]);
    advertisement.original_hash = advertisement.hash;
    advertisement
}

fn offer(manager: &mut ResourceManager, link: &mut Link, advertisement: &ResourceAdvertisement) {
    let packet = resource_packet(
        PacketContext::ResourceAdvrtisement,
        &advertisement.pack().expect("advertisement"),
        *link.id(),
    );
    manager.handle_packet(&packet, link);
}

#[test]
fn link_admission_refuses_oversized_resource_before_allocation() {
    let (mut manager, mut link) = manager_with_test_link();
    manager.set_link_admission(
        *link.id(),
        Some(LinkResourceAdmission { max_data_size: 32, max_concurrent: 8 }),
    );

    offer(&mut manager, &mut link, &advertisement(1, 33));
    assert!(manager.incoming.is_empty(), "over-budget Resource must not allocate a receiver");

    offer(&mut manager, &mut link, &advertisement(2, 32));
    assert_eq!(manager.incoming.len(), 1, "Resource at the budget is admitted");
}

#[test]
fn link_admission_bounds_concurrent_inbound_assemblies() {
    let (mut manager, mut link) = manager_with_test_link();
    manager.set_link_admission(
        *link.id(),
        Some(LinkResourceAdmission { max_data_size: 1024, max_concurrent: 1 }),
    );

    offer(&mut manager, &mut link, &advertisement(1, 16));
    offer(&mut manager, &mut link, &advertisement(2, 16));
    assert_eq!(manager.incoming.len(), 1, "second concurrent Resource is refused");

    manager.set_link_admission(*link.id(), None);
    offer(&mut manager, &mut link, &advertisement(3, 16));
    assert_eq!(manager.incoming.len(), 2, "clearing the budget restores default admission");
}

#[test]
fn link_admission_is_dropped_with_link_state() {
    let (mut manager, link) = manager_with_test_link();
    manager.set_link_admission(
        *link.id(),
        Some(LinkResourceAdmission { max_data_size: 1, max_concurrent: 1 }),
    );
    manager.remove_link_state(*link.id());
    assert!(manager.link_admission.is_empty());
}
