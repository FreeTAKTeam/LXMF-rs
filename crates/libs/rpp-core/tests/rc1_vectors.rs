//! RPP 0.3 RC1 wire-review vectors (`tests/fixtures/rc1.json`, release
//! v0.3.0-rc.1): manifests, signatures, commands, results, publications and
//! the codec/manifest negative cases.
mod common;

use common::*;
use rpp_core::cbor;
use rpp_core::control::{Control, RppResult};
use rpp_core::publication::{Publication, PublicationKind};
use rpp_core::{RppError, VerifiedManifest};

#[test]
fn fixture_identities_derive_publisher_inbox() {
    let fixtures = fixtures();
    for name in ["one", "second_owner"] {
        let owner = package_owner(&fixtures, name);
        let value = &fixtures["packages"][name]["value"];
        assert_eq!(owner.public().receive_destination().to_vec(), diag_bytes(&value["4"]));
    }
}

#[test]
fn every_package_rebuilds_byte_for_byte_and_verifies() {
    let fixtures = fixtures();
    for (name, case) in fixtures["packages"].as_object().expect("packages") {
        let expected = hex(&case["cbor_hex"]);
        assert_eq!(cbor::encode(&diag(&case["value"])), expected, "{name}: diagnostic value");
        let rebuilt = rebuild(&fixtures, name);
        assert_eq!(rebuilt.bytes, expected, "{name}: rebuilt manifest bytes");
        assert_eq!(rebuilt.manifest.package_id.to_vec(), hex(&case["package_id_hex"]), "{name}");
        assert_eq!(rebuilt.hash.to_vec(), hex(&case["manifest_hash_hex"]), "{name}");
        let verified = VerifiedManifest::verify(&expected, &binding(&fixtures, name)).expect(name);
        assert_eq!(verified, rebuilt);
    }
}

#[test]
fn same_content_under_distinct_owners_has_distinct_identity() {
    let fixtures = fixtures();
    let one = rebuild(&fixtures, "one");
    let other = rebuild(&fixtures, "second_owner");
    assert_eq!(one.manifest.files[0].digest, other.manifest.files[0].digest);
    assert_ne!(one.manifest.package_id, other.manifest.package_id);
    // A manifest never verifies under another owner's binding.
    let wrong = VerifiedManifest::verify(&one.bytes, &binding(&fixtures, "second_owner"));
    assert!(wrong.is_err());
}

#[test]
fn commands_results_and_additional_controls_roundtrip() {
    let fixtures = fixtures();
    for group in ["commands", "additional_controls"] {
        for (name, case) in fixtures[group].as_object().expect("controls") {
            let bytes = hex(&case["cbor_hex"]);
            assert_eq!(cbor::encode(&diag(&case["value"])), bytes, "{name}: diagnostic");
            let control = Control::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            if name != "ignore_noncritical" {
                assert_eq!(control.encode(), bytes, "{name}: re-encoding");
            }
        }
    }
    for (name, case) in fixtures["results"].as_object().expect("results") {
        let bytes = hex(&case["cbor_hex"]);
        let result = RppResult::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(result.encode(), bytes, "{name}");
        if let Some(reply) = result.value {
            let reply = Control::decode(&reply).expect("embedded reply");
            let request =
                Control::decode(&hex(&fixtures["commands"][name]["cbor_hex"])).expect("test step");
            assert_eq!(reply.header, request.header, "{name}: reply echoes request header");
        }
    }
}

#[test]
fn publication_references_verify_against_resume_manifest() {
    let fixtures = fixtures();
    let manifest = rebuild(&fixtures, "resume101");
    let owner = package_owner(&fixtures, "resume101").public();
    for (kind, expected) in [
        ("Package", PublicationKind::Package),
        ("Manifest", PublicationKind::Manifest),
        ("File", PublicationKind::File),
    ] {
        let bytes = hex(&fixtures["resume_publication"][kind]["cbor_hex"]);
        assert_eq!(cbor::encode(&diag(&fixtures["resume_publication"][kind]["value"])), bytes);
        let publication = Publication::verify(&bytes, &owner).expect(kind);
        assert_eq!(publication.kind, expected);
        publication.check_manifest(&manifest).expect(kind);
        let foreign = package_owner(&fixtures, "second_owner").public();
        assert_eq!(Publication::verify(&bytes, &foreign), Err(RppError::InvalidSignature));
    }
}

#[test]
fn codec_and_manifest_negative_cases() {
    let fixtures = fixtures();
    for case in fixtures["negative"].as_array().expect("negative") {
        let name = case["case"].as_str().expect("case");
        let expect = case["expect"].as_str().expect("expect");
        let input = hex(&case["input_hex"]);
        let error = match case["kind"].as_str().expect("kind") {
            "control" => match Control::decode(&input) {
                Err(error) => error.error,
                Ok(_) => continue, // policy-level case, see transfer tests
            },
            "manifest" => {
                VerifiedManifest::verify(&input, &binding(&fixtures, "resume101")).expect_err(name)
            }
            _ => continue, // resource / receipt cases run in the transfer tests
        };
        let matched = match expect {
            "malformed" => matches!(error, RppError::Malformed(_)),
            "noncanonical" => matches!(error, RppError::Noncanonical(_)),
            "unsupported_version" => matches!(error, RppError::UnsupportedVersion(_)),
            "unsupported_extension" => matches!(error, RppError::UnsupportedExtension(_)),
            "invalid_signature" => error == RppError::InvalidSignature,
            "invalid_selection" => error == RppError::InvalidSelection,
            other => panic!("{name}: unexpected decode failure for {other}: {error:?}"),
        };
        assert!(matched, "{name}: expected {expect}, got {error:?}");
    }
}
