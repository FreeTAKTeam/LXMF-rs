use alloc::vec;

use super::{Message, WireMessage};
use crate::constants::FIELD_TICKET;
use crate::stamp::{ticket_stamp, validate_stamp, COST_TICKET, TICKET_LENGTH};
use rns_core::identity::PrivateIdentity;

fn sample(timestamp: Option<f64>) -> Message {
    let mut message = Message::new();
    message.destination_hash = Some([0x11; 16]);
    message.source_hash = Some([0x22; 16]);
    message.set_title_from_string("title");
    message.set_content_from_string("hello");
    message.timestamp = timestamp;
    message
}

#[test]
fn the_message_id_needs_the_timestamp_and_excludes_the_stamp() {
    let mut message = sample(Some(1_700_000_000.5));
    let id = message.message_id().expect("id");

    message.stamp = Some(vec![0xaa; 8]);
    assert_eq!(
        message.message_id().expect("id"),
        id,
        "a stamp is not part of what it is derived from"
    );
    assert!(
        sample(None).message_id().is_err(),
        "an unset timestamp would be filled at pack time, after the id was needed"
    );
}

#[test]
fn the_message_id_is_the_one_the_packed_wire_carries() {
    let message = sample(Some(1_700_000_000.5));
    let signer = PrivateIdentity::new_from_name("delivery-stamp-id");

    let wire = message.to_wire(Some(&signer)).expect("packs");

    assert_eq!(
        WireMessage::unpack(&wire).expect("unpacks").message_id(),
        message.message_id().expect("id")
    );
}

#[test]
fn a_ticket_pays_the_stamp_without_a_search() {
    let mut message = sample(Some(1_700_000_000.5));
    let ticket = vec![0x5a; TICKET_LENGTH];

    let value = message.stamp_for_delivery(Some(16), Some(&ticket), || false).expect("stamped");

    let id = message.message_id().expect("id");
    assert_eq!(value, Some(COST_TICKET));
    assert_eq!(message.stamp.as_deref(), Some(ticket_stamp(&ticket, &id).as_slice()));
    assert_eq!(validate_stamp(message.stamp.as_deref(), &id, 16, &[ticket]), Some(COST_TICKET));
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn a_ticket_of_the_wrong_length_is_ignored_and_the_stamp_is_mined() {
    let mut message = sample(Some(1_700_000_000.5));

    let value = message.stamp_for_delivery(Some(2), Some(&[0x5a; 4]), || false).expect("stamped");

    let id = message.message_id().expect("id");
    assert!(value.is_some_and(|value| value >= 2));
    assert_eq!(validate_stamp(message.stamp.as_deref(), &id, 2, &[]), value);
}

#[test]
fn no_cost_and_no_ticket_means_no_stamp() {
    let mut message = sample(Some(1_700_000_000.5));

    assert_eq!(message.stamp_for_delivery(None, None, || false).expect("nothing owed"), None);
    assert!(message.stamp.is_none());
}

#[test]
#[cfg_attr(miri, ignore = "proof-of-work expansion is prohibitively slow under Miri")]
fn a_stopped_search_fails_rather_than_sending_unstamped() {
    let mut message = sample(Some(1_700_000_000.5));

    assert!(message.stamp_for_delivery(Some(8), None, || true).is_err());
    assert!(message.stamp.is_none());
}

#[test]
fn include_ticket_writes_the_reference_field_and_replaces_an_earlier_one() {
    let mut message = sample(Some(1_700_000_000.5));
    let key = rmpv::Value::from(FIELD_TICKET);

    message.include_ticket(1_900_000_000.0, &[0xab; TICKET_LENGTH]);
    message.include_ticket(1_900_000_100.0, &[0xcd; TICKET_LENGTH]);

    let fields = message.fields.as_ref().and_then(rmpv::Value::as_map).expect("a map");
    let tickets: vec::Vec<_> = fields.iter().filter(|(field, _)| *field == key).collect();
    assert_eq!(tickets.len(), 1, "one ticket field, the latest");
    assert_eq!(
        tickets[0].1,
        rmpv::Value::Array(vec![
            rmpv::Value::F64(1_900_000_100.0),
            rmpv::Value::Binary(vec![0xcd; TICKET_LENGTH])
        ])
    );

    let mut with_other_fields = sample(Some(1.0));
    with_other_fields.fields =
        Some(rmpv::Value::Map(vec![(rmpv::Value::from(1u8), rmpv::Value::from("kept"))]));
    with_other_fields.include_ticket(2.0, &[0xab; TICKET_LENGTH]);
    assert_eq!(
        with_other_fields.fields.as_ref().and_then(rmpv::Value::as_map).map(|map| map.len()),
        Some(2)
    );

    let mut not_a_map = sample(Some(1.0));
    not_a_map.fields = Some(rmpv::Value::Nil);
    not_a_map.include_ticket(2.0, &[0xab; TICKET_LENGTH]);
    assert!(not_a_map.fields.as_ref().and_then(rmpv::Value::as_map).is_some());
}
