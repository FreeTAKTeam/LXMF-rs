//! Offline MeshChat profile inspection and identity-checked history migration.
use rns_rpc::MessagesStore;
use rns_transport::{
    destination::{DestinationName, SingleInputDestination},
    identity::PrivateIdentity,
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{fs, io, path::Path};

fn failure(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

pub fn inspect(source_identity: &Path) -> io::Result<Value> {
    let key = fs::read(source_identity)?;
    inspect_key(source_identity, &key)
}

fn inspect_key(source_identity: &Path, key: &[u8]) -> io::Result<Value> {
    let identity = PrivateIdentity::from_private_key_bytes(key)
        .map_err(|_| failure("MeshChat identity must be a 64-byte private identity"))?;
    let identity_hash = hex::encode(identity.address_hash().as_slice());
    let destination =
        SingleInputDestination::new(identity, DestinationName::new("lxmf", "delivery"));
    let local = hex::encode(destination.desc.address_hash.as_slice());
    let source_db = source_identity
        .parent()
        .ok_or_else(|| failure("Identity has no profile folder"))?
        .join("identities")
        .join(&identity_hash)
        .join("database.db");
    let (database_path, count) = if source_db.try_exists()? {
        let conn = Connection::open_with_flags(&source_db, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(failure)?;
        conn.prepare("SELECT id, hash, source_hash, destination_hash, is_incoming, title, content, timestamp, fields, state, progress FROM lxmf_messages LIMIT 0")
            .map_err(|error| failure(format!("Not a supported MeshChat message database: {error}")))?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM lxmf_messages", [], |row| row.get(0))
            .map_err(failure)?;
        (Some(source_db), count)
    } else {
        (None, 0)
    };
    Ok(json!({"identity_hash":identity_hash,"lxmf_address_hash":local,
        "database_path":database_path,"message_count":count}))
}

pub fn import(
    source_identity: &Path,
    target_identity: &Path,
    target_db: &Path,
) -> io::Result<Value> {
    let source_key = fs::read(source_identity)?;
    if fs::read(target_identity)? != source_key {
        return Err(failure(
            "MeshChat identity does not match this YARA profile; its identity was not replaced",
        ));
    }
    let mut summary = inspect_key(source_identity, &source_key)?;
    if let Some(source_db) = summary["database_path"].as_str() {
        let source_db = Path::new(source_db);
        if target_db.try_exists()? && source_db.canonicalize()? == target_db.canonicalize()? {
            return Err(failure("Source and target message databases must be different"));
        }
        let store = MessagesStore::open(target_db).map_err(failure)?;
        let result = store
            .import_meshchat_history(
                source_db,
                summary["lxmf_address_hash"]
                    .as_str()
                    .ok_or_else(|| failure("Missing local destination"))?,
            )
            .map_err(|error| {
                failure(format!("MeshChat history import failed; no messages were added: {error}"))
            })?;
        summary
            .as_object_mut()
            .ok_or_else(|| failure("Invalid import summary"))?
            .extend(result.as_object().ok_or_else(|| failure("Invalid history summary"))?.clone());
    } else {
        summary["imported"] = json!(0);
        summary["skipped"] = json!(0);
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meshchat_inspection_derives_profile_path_and_rejects_invalid_or_mismatched_keys() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("identity");
        fs::write(&source, [1u8; 64]).unwrap();
        let summary = inspect(&source).unwrap();
        assert!(summary["database_path"].is_null());
        let database = root
            .path()
            .join("identities")
            .join(summary["identity_hash"].as_str().unwrap())
            .join("database.db");
        fs::create_dir_all(database.parent().unwrap()).unwrap();
        let conn = Connection::open(&database).unwrap();
        conn.execute_batch("CREATE TABLE lxmf_messages (id INTEGER, hash TEXT, source_hash TEXT, destination_hash TEXT, is_incoming INTEGER, title TEXT, content TEXT, timestamp REAL, fields TEXT, state TEXT, progress REAL)").unwrap();
        assert_eq!(inspect(&source).unwrap()["database_path"], database.to_string_lossy().as_ref());
        let target = root.path().join("target-key");
        fs::write(&target, [2u8; 64]).unwrap();
        let target_db = root.path().join("target.db");
        assert!(import(&source, &target, &target_db)
            .unwrap_err()
            .to_string()
            .contains("does not match"));
        assert!(!target_db.exists());
        fs::write(&source, b"invalid").unwrap();
        assert!(inspect(&source).is_err());
    }
}
