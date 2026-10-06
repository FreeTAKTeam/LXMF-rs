#[test]
fn propagation_offer_metadata_uses_covering_index_without_reading_payloads() {
    let store = MessagesStore::in_memory().expect("store");
    for (id, destination, size) in
        [("aa", "abcd", 20), ("bb", "abcd", 10), ("cc", "abcd", 10), ("dd", "ffff", 5)]
    {
        store
            .upsert_propagation_entry(&PropagationEntryRecord {
                transient_id: id.into(),
                destination: destination.into(),
                payload_hex: "aa".repeat(100_000),
                received_at: 123,
                size_bytes: size,
                stamp_value: None,
            })
            .expect("entry");
    }
    let expected = vec![("bb".into(), 10), ("cc".into(), 10), ("aa".into(), 20)];
    assert_eq!(
        store.list_propagation_entry_metadata_for_destination(" ABCD ").expect("metadata"),
        expected
    );
    store.with_read_conn(|conn| {
            let mut query = conn.prepare("EXPLAIN QUERY PLAN SELECT transient_id, size_bytes FROM propagation_entries WHERE destination = ?1 ORDER BY size_bytes ASC, transient_id ASC")?;
            let plan = query.query_map(["abcd"], |row| row.get::<_, String>(3))?.collect::<Result<Vec<_>, _>>()?.join(" ");
            assert!(plan.contains("USING COVERING INDEX idx_propagation_entries_destination_size"), "{plan}");
            Ok(())
        }).expect("covering index");
    // Invalid payload storage remains visible to fetch callers, but metadata
    // listing must not load or decode contents it does not use.
    store
        .with_write_conn(|conn| {
            conn.execute(
                "UPDATE propagation_entries SET payload_hex=x'ff' WHERE transient_id='aa'",
                [],
            )?;
            Ok(())
        })
        .expect("malformed payload");
    assert_eq!(
        store.list_propagation_entry_metadata_for_destination("abcd").expect("metadata only"),
        expected
    );
    assert!(store.get_propagation_entry("aa").is_err());
}
