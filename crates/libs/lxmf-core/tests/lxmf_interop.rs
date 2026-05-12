use lxmf_core::announce::{parse_announce_slots, AnnounceSlot};
use lxmf_core::{Payload, TransportMethod, WireMessage};
use rmpv::Value;
use std::fs;
use serde_json::Value as JsonValue;

fn fixture_dir() -> &'static str { "tests/fixtures/lxmf_interop" }
fn fixture_bytes(name: &str) -> Vec<u8> {
    let raw = fs::read_to_string(format!("{}/fixtures.json", fixture_dir())).unwrap();
    let parsed: JsonValue = serde_json::from_str(&raw).unwrap();
    let hex = parsed["fixtures"][name]["hex"].as_str().unwrap();
    hex::decode(hex).unwrap()
}

fn deterministic_wire(stamp: Option<Vec<u8>>, fields: Option<Value>) -> WireMessage {
    let payload = Payload::new(1_773_999_000.25, Some(b"interop-content".to_vec()), Some(b"interop-title".to_vec()), fields, stamp);
    let mut wire = WireMessage::new([0x11; 16], [0x22; 16], payload);
    wire.signature = Some([0x33; 64]);
    wire
}
fn first_diff(a: &[u8], b: &[u8]) -> String { for i in 0..a.len().min(b.len()){ if a[i]!=b[i]{ return format!("byte {} differs: {:02x} != {:02x}",i,a[i],b[i]);}} format!("length differs: {} != {}",a.len(),b.len()) }

#[test]
fn pack_stamp_roundtrip_matches_reference_fixture() { let wire=deterministic_wire(Some(vec![0x44;32]),None); let packed=wire.pack().unwrap(); let fixture=fixture_bytes("direct_with_stamp"); assert_eq!(packed,fixture,"{}",first_diff(&packed,&fixture)); let reparsed=WireMessage::unpack(&fixture).unwrap(); assert_eq!(reparsed.payload.stamp.as_ref().unwrap().as_ref(),vec![0x44;32].as_slice()); let repacked=reparsed.pack().unwrap(); assert_eq!(repacked,fixture,"{}",first_diff(&repacked,&fixture)); }

#[test]
fn rust_fixture_matches_python_binary_envelope() { let packed=deterministic_wire(None,None).pack().unwrap(); let fixture=fixture_bytes("direct_no_stamp"); assert_eq!(packed,fixture,"{}",first_diff(&packed,&fixture)); }

#[test]
fn python_fixture_parses_and_repacks_without_stamp_mutation() { let fixture=fixture_bytes("propagation_with_stamp"); let (ts,entries):(f64,Vec<serde_bytes::ByteBuf>)=rmp_serde::from_slice(&fixture).unwrap(); assert_eq!(ts,1_773_999_001.5); let stamp=&entries[0][entries[0].len()-32..]; assert_eq!(stamp,vec![0x55;32].as_slice()); let repacked=WireMessage::pack_propagation_envelope(ts,&entries[0][..entries[0].len()-32],Some(stamp)).unwrap(); assert_eq!(repacked,fixture,"{}",first_diff(&repacked,&fixture)); }

#[derive(Default)] struct MockHub{accepted:bool,queued:bool,retried:bool}
fn route_message(h:&mut MockHub,m:TransportMethod,s:Option<&[u8]>) {match m{TransportMethod::Direct=>h.accepted=true,TransportMethod::Propagated=>{if s.is_some(){h.accepted=true;h.queued=true;h.retried=true;}}, _=>{}}}

#[test]
fn direct_message_is_not_queued_for_propagation(){let env=deterministic_wire(None,None).pack().unwrap(); let mut h=MockHub::default(); route_message(&mut h,TransportMethod::Direct,None); assert!(!env.is_empty()); assert!(h.accepted && !h.queued);} 

#[test]
fn propagation_message_with_valid_stamp_is_queued(){let wire=deterministic_wire(Some(vec![0x44;32]),None); let _=wire.pack_propagation_with_options_and_rng(&rns_core::identity::PrivateIdentity::new_from_name("rx").as_identity(),1_773_999_001.5,Some(&[0x55;32]),rand_core::OsRng).unwrap(); let mut h=MockHub::default(); route_message(&mut h,TransportMethod::Propagated,Some(&[0x55;32])); assert!(h.accepted && h.queued && h.retried);} 

#[test] fn announce_slots_parse_known_capabilities(){let p=parse_announce_slots(&[0x01,0x03,b'a',b'b',b'c',0x05,0x01,0x09]).unwrap(); assert_eq!(p[0],AnnounceSlot{id:0x01,value:b"abc".to_vec()});}
#[test] fn announce_parser_handles_unknown_tlv_without_panic(){assert_eq!(parse_announce_slots(&[0x99,0x02,0xAA,0xBB]).unwrap()[0].id,0x99);}
#[test] fn announce_parser_rejects_truncated_slot(){assert!(parse_announce_slots(&[0x01,0x05,0xAA]).is_err());}

#[test]
fn regenerate_fixtures_when_env_set(){ if std::env::var("LXMF_REGEN_FIXTURES").ok().as_deref()!=Some("1"){return;} fs::create_dir_all(fixture_dir()).unwrap(); let no_stamp=deterministic_wire(None,None).pack().unwrap(); let with_stamp=deterministic_wire(Some(vec![0x44;32]),None).pack().unwrap(); let fields=Some(Value::Map(vec![(Value::Integer(99.into()),Value::String("meta".into()))])); let with_meta=deterministic_wire(Some(vec![0x44;32]),fields).pack().unwrap(); let (prop,_) = deterministic_wire(Some(vec![0x44;32]),None).pack_propagation_with_options_and_rng(&rns_core::identity::PrivateIdentity::new_from_name("rx").as_identity(),1_773_999_001.5,Some(&[0x55;32]),rand_core::OsRng).unwrap(); let malformed=vec![0x91,0xC0]; fs::write(format!("{}/fixtures.json",fixture_dir()),serde_json::to_vec_pretty(&serde_json::json!({"fixtures":{"direct_no_stamp":{"hex":hex::encode(no_stamp)},"direct_with_stamp":{"hex":hex::encode(with_stamp)},"direct_with_metadata":{"hex":hex::encode(with_meta)},"propagation_with_stamp":{"hex":hex::encode(prop)},"malformed_missing_stamp_tail":{"hex":hex::encode(malformed)}}})).unwrap()).unwrap(); }
