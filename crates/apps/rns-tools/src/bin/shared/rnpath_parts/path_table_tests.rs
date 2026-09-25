#[test]
fn path_table_rows_sort_by_interface_then_hop_count() {
    let mut rows = vec![
        json!({ "interface": "uplink-b", "hops": 1 }),
        json!({ "interface": "uplink-a", "hops": 2 }),
        json!({ "interface": "uplink-a", "hops": 1 }),
    ];

    sort_path_rows(&mut rows);

    assert_eq!(rows[0]["hops"], 1);
    assert_eq!(rows[1]["hops"], 2);
    assert_eq!(rows[2]["interface"], "uplink-b");
}

#[test]
fn json_path_table_does_not_filter_destination_like_python() {
    let mut rows = vec![
        json!({ "hash": "00112233445566778899aabbccddeeff" }),
        json!({ "hash": "ffeeddccbbaa99887766554433221100" }),
    ];

    assert!(filter_path_rows(
        &mut rows,
        Some("00112233445566778899aabbccddeeff"),
        true,
    ));
    assert_eq!(rows.len(), 2);
}

#[test]
fn human_path_table_filters_destination_and_reports_absence() {
    let mut rows = vec![
        json!({ "hash": "00112233445566778899aabbccddeeff" }),
        json!({ "hash": "ffeeddccbbaa99887766554433221100" }),
    ];

    assert!(filter_path_rows(
        &mut rows,
        Some("00112233445566778899aabbccddeeff"),
        false,
    ));
    assert_eq!(rows.len(), 1);
    assert!(!filter_path_rows(&mut rows, Some("ffeeddccbbaa99887766554433221100"), false));
}

#[test]
fn path_table_expiry_matches_reference_local_timestamp_shape() {
    assert_eq!(
        format_path_timestamp_at_offset(0.0, time::UtcOffset::UTC).as_deref(),
        Some("1970-01-01 00:00:00")
    );
}
