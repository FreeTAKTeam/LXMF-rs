//! RDO owner binding v1 vectors: Destinations, revisions and owner operations.

mod common;

use common::*;
use rdo_core::lookup::{announce_app_data, object_destination};
use rdo_core::{OpResult, Owner, Policy, PublicIdentity, RdoError, Request, Revision, Status};
use serde_json::Value as Json;

/// "Both identified principals may read. Only editor may propose."
struct FixturePolicy {
    reader: PublicIdentity,
    editor: PublicIdentity,
}

impl Policy for FixturePolicy {
    fn may_read(&self, who: &PublicIdentity) -> bool {
        *who == self.reader || *who == self.editor
    }
    fn may_propose(&self, who: &PublicIdentity) -> bool {
        *who == self.editor
    }
}

#[test]
fn every_binding_reconstructs_its_destination_and_app_data() {
    let fixtures = fixtures();
    let owner = owner(&fixtures).public();
    assert_eq!(owner, public(&fixtures, "owner_public_hex"));
    let bindings = fixtures["bindings"].as_array().expect("bindings");
    assert!(bindings.len() >= 7, "Object, Property, Relationship and M2 examples");
    for binding in bindings {
        let token = fixed(hex(&binding["token_hex"]));
        let destination: [u8; 16] = fixed(hex(&binding["destination_hex"]));
        assert_eq!(object_destination(&owner, &token), destination, "{}", binding["label"]);
        let packet = hex(&binding["announce_packet_hex"]);
        assert!(packet.ends_with(&announce_app_data(&token)), "{}", binding["label"]);
    }
}

#[test]
fn revisions_verify_and_resign_byte_for_byte() {
    let fixtures = fixtures();
    let owner = owner(&fixtures);
    let mut previous: Option<Revision> = None;
    for name in ["property_genesis", "property_successor"] {
        let case = &fixtures["revisions"][name];
        let bytes = hex(&case["cbor_hex"]);
        let revision = Revision::verify(&bytes, &owner.public()).expect(name);
        assert_eq!(revision.id.to_vec(), hex(&case["revision_hash_hex"]), "{name}");
        let resigned = Revision::sign(
            &owner,
            revision.object,
            revision.parent,
            &revision.type_iri,
            revision.body.clone(),
        )
        .expect(name);
        assert_eq!(resigned.bytes, bytes, "{name}: deterministic signature");
        if let Some(previous) = &previous {
            previous.check_successor(&revision).expect("successor names the head");
        }
        previous = Some(revision);
    }
    let relationship = hex(&fixtures["revisions"]["relationship_genesis"]["cbor_hex"]);
    Revision::verify(&relationship, &owner.public()).expect("relationship genesis");
}

#[test]
fn invalid_revision_is_refused() {
    let fixtures = fixtures();
    let case = &fixtures["invalid_revision"];
    let owner = PublicIdentity(fixed(hex(&case["expected_owner_public_hex"])));
    let error = Revision::verify(&hex(&case["cbor_hex"]), &owner).expect_err("invalid revision");
    assert_eq!(case["expected"].as_str(), Some("reject_signature"), "fixture class");
    assert_eq!(error, RdoError::InvalidSignature);
}

fn history(fixtures: &Json, head: &[u8]) -> Vec<Revision> {
    let owner = owner(fixtures).public();
    let mut chain = Vec::new();
    for name in ["property_genesis", "property_successor"] {
        let bytes = hex(&fixtures["revisions"][name]["cbor_hex"]);
        chain.push(Revision::verify(&bytes, &owner).expect(name));
        if chain.last().expect("pushed").id.as_slice() == head {
            return chain;
        }
    }
    panic!("initial head is not in the fixture chain");
}

#[test]
fn owner_operations_match_every_case() {
    let fixtures = fixtures();
    let now = fixtures["fixture_time"].as_u64().expect("fixture time");
    let policy = FixturePolicy {
        reader: public(&fixtures, "reader_public_hex"),
        editor: public(&fixtures, "editor_public_hex"),
    };
    let operations = fixtures["operations"].as_array().expect("operations");
    let find = |name: &str| operations.iter().find(|op| op["case"] == name).expect("case");
    let bindings = fixtures["bindings"].as_array().expect("bindings");
    let property = bindings.iter().find(|b| b["label"] == "Property").expect("Property binding");
    let link: [u8; 16] = fixed(hex(&property["destination_hex"]));
    for case in operations {
        let name = case["case"].as_str().expect("case name");
        let principal = public(
            &fixtures,
            &format!("{}_public_hex", case["principal"].as_str().expect("principal")),
        );
        let mut start = hex(&case["initial_head_hex"]);
        if let Some(prior) = case["prior_request"].as_str() {
            start = hex(&find(prior)["initial_head_hex"]);
        }
        let mut owner = Owner::new(owner(&fixtures), history(&fixtures, &start)).expect(name);
        assert_eq!(owner.object(), link, "{name}: operations target the Property object");
        if let Some(prior) = case["prior_request"].as_str() {
            let request = hex(&find(prior)["request"]["cbor_hex"]);
            owner.handle(Some(&principal), &link, &request, now, &policy).expect("prior result");
        }
        let commits_before = owner.history().len();
        let request = hex(&case["request"]["cbor_hex"]);
        assert_eq!(Request::decode(&request).expect(name).encode(), request, "{name}: request");
        let result = owner.handle(Some(&principal), &link, &request, now, &policy).expect(name);
        assert_eq!(result, hex(&case["result"]["cbor_hex"]), "{name}: result bytes");
        let expected_commits = case["new_commits"].as_u64().unwrap_or(0) as usize;
        assert_eq!(owner.history().len() - commits_before, expected_commits, "{name}: commits");
        assert_eq!(owner.head().id.to_vec(), head_after(case), "{name}: head");
    }
}

/// The head after this case: the new revision, or the named initial head.
fn head_after(case: &Json) -> Vec<u8> {
    let result = OpResult::decode(&hex(&case["result"]["cbor_hex"])).expect("result");
    match (result.status, case["new_commits"].as_u64()) {
        (Status::Applied, Some(1)) => result.version.expect("new head").to_vec(),
        _ => hex(&case["initial_head_hex"]),
    }
}

#[test]
fn unidentified_undecodable_and_expired_requests() {
    let fixtures = fixtures();
    let now = fixtures["fixture_time"].as_u64().expect("fixture time");
    let editor = public(&fixtures, "editor_public_hex");
    let policy = FixturePolicy { reader: editor, editor };
    let genesis = hex(&fixtures["revisions"]["property_genesis"]["revision_hash_hex"]);
    let mut owner = Owner::new(owner(&fixtures), history(&fixtures, &genesis)).expect("owner");
    let link = owner.object();
    let read = Request::read([7; 16], link, now + 60).encode();
    assert!(owner.handle(None, &link, &read, now, &policy).is_none(), "unidentified is dropped");
    assert!(owner.handle(Some(&editor), &link, b"\xff", now, &policy).is_none());
    let late = owner.handle(Some(&editor), &link, &read, now + 60, &policy).expect("result");
    assert_eq!(OpResult::decode(&late).expect("result").status, Status::Rejected);
    let too_long = Request::read([8; 16], link, now + 301).encode();
    let result = owner.handle(Some(&editor), &link, &too_long, now, &policy).expect("result");
    assert_eq!(OpResult::decode(&result).expect("result").status, Status::Rejected);
    let wrong_link = owner.handle(
        Some(&editor),
        &[9; 16],
        &Request::read([9; 16], link, now + 60).encode(),
        now,
        &policy,
    );
    assert_eq!(
        OpResult::decode(&wrong_link.expect("result")).expect("result").status,
        Status::Rejected
    );
}
