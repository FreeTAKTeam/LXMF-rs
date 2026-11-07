use crate::{identities, prepared::delivery_address, Error, OwnedStore, MAX_PENDING};
use rns_core::identity::Identity;

impl OwnedStore {
    /// The embedding library obtains these keys from a verified native announce.
    pub fn remember_recipient(
        &mut self,
        destination: &[u8; 16],
        identity: &Identity,
        now: i64,
    ) -> Result<(), Error> {
        self.ensure_running()?;
        if delivery_address(identity) != *destination {
            return Err(Error::UnresolvedRecipient);
        }
        let tx = self.conn.unchecked_transaction()?;
        identities::upsert(
            &tx,
            &hex::encode(destination),
            &hex::encode(identity.public_key_bytes()),
            &hex::encode(identity.verifying_key_bytes()),
            now,
        )?;
        // Bound key metadata separately; pruning never changes message custody.
        tx.execute("DELETE FROM announce_identities WHERE peer IN
            (SELECT peer FROM announce_identities ORDER BY updated_at DESC,peer DESC LIMIT -1 OFFSET ?1)",
            [MAX_PENDING as i64])?;
        tx.commit()?;
        Ok(())
    }

    pub fn recipient_identity(&self, destination: &[u8; 16]) -> Result<Option<Identity>, Error> {
        let Some((public, verifying)) = identities::keys(&self.conn, &hex::encode(destination))?
        else {
            return Ok(None);
        };
        let public = hex::decode(public).map_err(|_| Error::UnresolvedRecipient)?;
        let verifying = hex::decode(verifying).map_err(|_| Error::UnresolvedRecipient)?;
        let identity = Identity::try_new_from_slices(&public, &verifying)
            .map_err(|_| Error::UnresolvedRecipient)?;
        if delivery_address(&identity) != *destination {
            return Err(Error::UnresolvedRecipient);
        }
        Ok(Some(identity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rns_core::identity::PrivateIdentity;

    #[test]
    fn recipient_keys_survive_restart_and_reject_invalid_bindings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("custody.db");
        let owner = PrivateIdentity::new_from_name("cache-owner");
        let recipient = PrivateIdentity::new_from_name("cache-recipient");
        let address = delivery_address(recipient.as_identity());
        let mut store =
            OwnedStore::open(&path, owner.clone(), crate::StoreLimits::default()).unwrap();
        assert!(matches!(
            store.remember_recipient(&[0; 16], recipient.as_identity(), 1),
            Err(Error::UnresolvedRecipient)
        ));
        store.remember_recipient(&address, recipient.as_identity(), 1).unwrap();
        drop(store);
        let store = OwnedStore::open(path, owner, crate::StoreLimits::default()).unwrap();
        assert_eq!(
            store.recipient_identity(&address).unwrap().unwrap().public_key_bytes(),
            recipient.as_identity().public_key_bytes()
        );
        let other = PrivateIdentity::new_from_name("wrong-key");
        identities::upsert(
            &store.conn,
            &hex::encode(address),
            &hex::encode(other.as_identity().public_key_bytes()),
            &hex::encode(other.as_identity().verifying_key_bytes()),
            2,
        )
        .unwrap();
        assert!(matches!(store.recipient_identity(&address), Err(Error::UnresolvedRecipient)));
        identities::upsert(&store.conn, &hex::encode(address), "broken", "broken", 3).unwrap();
        assert!(matches!(store.recipient_identity(&address), Err(Error::UnresolvedRecipient)));
    }

    #[test]
    fn recipient_cache_is_bounded_without_touching_custody() {
        let dir = tempfile::tempdir().unwrap();
        let owner = PrivateIdentity::new_from_name("bounded-cache-owner");
        let mut store = OwnedStore::open(
            dir.path().join("custody.db"),
            owner.clone(),
            crate::StoreLimits::default(),
        )
        .unwrap();
        let tx = store.conn.unchecked_transaction().unwrap();
        for index in 0..MAX_PENDING {
            identities::upsert(&tx, &format!("{index:032x}"), "00", "00", 0).unwrap();
        }
        tx.commit().unwrap();
        let address = delivery_address(owner.as_identity());
        store.remember_recipient(&address, owner.as_identity(), 1).unwrap();
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM announce_identities", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, MAX_PENDING as i64);
        assert!(store.recipient_identity(&address).unwrap().is_some());
        assert!(store.status(1).unwrap().is_empty());
    }
}
