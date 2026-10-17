use lxmf_core::announce::{
    pn_announce_data_is_valid, pn_name_from_app_data, pn_stamp_cost_from_app_data,
};

#[test]
fn propagation_metadata_matches_lxmf_1_2_0_acceptance() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../test-support/fixtures/lxmf_1_2_0_metadata.json"))
            .expect("fixture");
    for case in fixture["cases"].as_array().expect("cases") {
        let bytes = hex::decode(case["wire_hex"].as_str().expect("wire hex")).expect("hex");
        assert_eq!(
            pn_announce_data_is_valid(&bytes),
            case["valid"].as_bool().expect("valid"),
            "{}",
            case["name"]
        );
        assert_eq!(
            pn_name_from_app_data(Some(&bytes)).as_deref(),
            case["display_name"].as_str(),
            "{}",
            case["name"]
        );
        assert_eq!(
            pn_stamp_cost_from_app_data(Some(&bytes)),
            case["stamp_cost"].as_i64(),
            "{}",
            case["name"]
        );
    }
}
