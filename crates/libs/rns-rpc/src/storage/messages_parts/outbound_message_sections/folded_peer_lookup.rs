    #[test]
    fn folded_peer_lookup_uses_index_and_preserves_legacy_case_variant_marks() {
        let path = std::env::temp_dir().join(format!(
            "lxmf-folded-peer-{}-{}.db", std::process::id(), now_unix_secs()
        ));
        let connection = Connection::open(&path).expect("legacy database");
        connection.execute_batch(
            "CREATE TABLE propagation_peer_entries (
                peer TEXT NOT NULL, transient_id TEXT NOT NULL, state TEXT NOT NULL,
                updated_at INTEGER NOT NULL, PRIMARY KEY(peer, transient_id)
             );",
        ).expect("legacy schema");
        for (peer, id, state) in [
            ("Peer-Mixed", "a1", "handled"),
            ("peer-mixed", "a1", "unhandled"),
            ("PEER-MIXED", "a2", "transferred"),
            ("Peer-Mixed", "a3", "received"),
            ("Peer-Mixed", "a4", "transfer_limited"),
            ("Peer-Mixed", "a5", "unhandled"),
            ("other-peer", "a6", "handled"),
        ] {
            connection.execute(
                "INSERT INTO propagation_peer_entries VALUES (?1, ?2, ?3, 123)",
                params![peer, id, state],
            ).expect("legacy mark");
        }
        let read_rows = |connection: &Connection| {
            let mut statement = connection.prepare(
                "SELECT peer,transient_id,state,updated_at FROM propagation_peer_entries
                 ORDER BY peer,transient_id",
            ).expect("rows");
            statement.query_map([], |row| Ok((
                row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, i64>(3)?,
            ))).expect("read").collect::<Result<Vec<_>, _>>().expect("collect")
        };
        let before = read_rows(&connection);
        drop(connection);
        for _ in 0..2 {
            let store = MessagesStore::open(&path).expect("initialize or reopen");
            for id in ["a1", "a2", "a3", "a4"] {
                assert!(store.peer_completed_propagation_mark_exists("pEeR-mIxEd", id).expect("completed"));
            }
            for (peer, id) in [("peer-mixed", "a5"), ("peer-mixed", "a6"), ("peer-mixed", "missing"), ("other-peer", "a1")] {
                assert!(!store.peer_completed_propagation_mark_exists(peer, id).expect("negative"));
            }
            store.with_read_conn(|connection| {
                let unique: bool = connection.query_row(
                    "SELECT \"unique\" FROM pragma_index_list('propagation_peer_entries')
                     WHERE name='idx_propagation_peer_entries_folded_lookup'",
                    [], |row| row.get(0),
                )?;
                assert!(!unique, "historical case variants must remain distinct raw rows");
                let mut statement = connection.prepare(
                    "EXPLAIN QUERY PLAN SELECT EXISTS(
                        SELECT 1 FROM propagation_peer_entries
                        WHERE LOWER(peer)=LOWER(?1) AND transient_id=?2
                          AND state IN ('handled','transferred','received','transfer_limited')
                        LIMIT 1)",
                )?;
                let plan = statement.query_map(params!["PEER-MIXED", "missing"], |row| row.get::<_, String>(3))?
                    .collect::<Result<Vec<_>, _>>()?.join(" ");
                assert!(plan.contains("SEARCH propagation_peer_entries USING") && plan.contains("idx_propagation_peer_entries_folded_lookup"), "{plan}");
                assert_eq!(read_rows(connection), before);
                Ok(())
            }).expect("indexed query and preserved rows");
            drop(store);
        }
        std::fs::remove_file(path).expect("cleanup");
    }
