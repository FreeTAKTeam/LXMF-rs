#[cfg(test)]
mod tests {
    use super::{
        OperationEntry, OperationKind, OperationRegistry, RegistryError, TransportFamily,
        TransportVariant,
    };
    use crate::app::EnvelopeKind;

    #[test]
    fn built_in_registry_canonicalizes_aliases() {
        let registry = OperationRegistry::built_in();
        assert_eq!(
            registry.canonicalize("sdk_identity_list_v2").expect("canonical operation id").as_str(),
            "app.identity.list"
        );
        assert_eq!(
            registry.get("sdk_poll_events_v2").expect("entry").id.as_str(),
            "app.event.poll"
        );
        assert_eq!(
            registry.get("sdk_paper_encode_v2").expect("entry").id.as_str(),
            "app.paper.encode"
        );
        assert_eq!(
            registry.canonicalize("sdk_paper_decode_v2").expect("canonical id").as_str(),
            "app.paper.decode"
        );
    }

    #[test]
    fn merged_registry_supports_custom_operations() {
        let registry = OperationRegistry::built_in()
            .merged([OperationEntry::new(
                "vendor.example.custom",
                "custom",
                OperationKind::Command,
                TransportVariant::Extension,
                "Custom vendor command.",
            )
            .with_alias("vendor/custom")])
            .expect("merged registry");

        assert!(registry.supports("vendor/custom"));
        assert_eq!(
            registry.canonicalize("vendor/custom").expect("canonical custom id").as_str(),
            "vendor.example.custom"
        );
    }

    #[test]
    fn rejects_duplicate_aliases() {
        let err = OperationRegistry::new([
            OperationEntry::new(
                "app.one",
                "test",
                OperationKind::Query,
                TransportVariant::App,
                "one",
            )
            .with_alias("dup"),
            OperationEntry::new(
                "app.two",
                "test",
                OperationKind::Query,
                TransportVariant::App,
                "two",
            )
            .with_alias("dup"),
        ])
        .expect_err("duplicate alias should fail");

        assert!(matches!(err, RegistryError::DuplicateAlias { alias, .. } if alias == "dup"));
    }

    #[test]
    fn deserialized_registry_rebuilds_lookup_indexes() {
        let json = serde_json::to_string(OperationRegistry::built_in()).expect("registry json");
        let registry: OperationRegistry = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(
            registry.canonicalize("sdk_status_v2").expect("canonical delivery status id").as_str(),
            "app.delivery.status"
        );
        assert!(registry.supports("sdk_snapshot_v2"));
    }

    #[test]
    fn resolve_reports_alias_and_transport_family() {
        let registry = OperationRegistry::built_in();
        let resolved = registry.resolve("sdk_poll_events_v2").expect("resolved alias");

        assert_eq!(resolved.canonical_id.as_str(), "app.event.poll");
        assert_eq!(resolved.alias.as_deref(), Some("sdk_poll_events_v2"));
        assert_eq!(resolved.entry.transport_family(), TransportFamily::Rpc);
        assert_eq!(resolved.entry.expected_envelope_kind(), EnvelopeKind::Query);
        assert!(resolved.entry.accepts_envelope_kind(&EnvelopeKind::Query));
        assert!(!resolved.entry.accepts_envelope_kind(&EnvelopeKind::Command));
    }

    #[test]
    fn registry_groups_entries_for_catalog_views() {
        let registry = OperationRegistry::built_in();
        let grouped = registry.entries_by_group();

        assert!(grouped.contains_key("runtime"));
        assert!(grouped.contains_key("attachments"));
        assert!(grouped.contains_key("markers"));
        assert!(grouped.contains_key("paper"));
        assert!(grouped.contains_key("telemetry"));
        assert!(grouped.contains_key("topics"));
        assert!(grouped.contains_key("voice"));
        assert!(grouped
            .get("identity")
            .expect("identity group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.identity.list"));
        assert!(grouped
            .get("attachments")
            .expect("attachments group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.attachment.store"));
        assert!(grouped
            .get("topics")
            .expect("topics group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.topic.create"));
        assert!(grouped
            .get("telemetry")
            .expect("telemetry group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.telemetry.query"));
        assert!(grouped
            .get("markers")
            .expect("markers group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.marker.create"));
        assert!(grouped
            .get("paper")
            .expect("paper group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.paper.encode"));
        assert!(grouped
            .get("voice")
            .expect("voice group")
            .iter()
            .any(|entry| entry.id.as_str() == "app.voice.session.open"));
    }

    #[test]
    fn r3akt_style_catalog_aliases_roundtrip_through_registry_json() {
        let registry = OperationRegistry::new([
            OperationEntry::new(
                "mission.join",
                "Core Discovery and Session",
                OperationKind::Command,
                TransportVariant::Extension,
                "Register the sender LXMF destination with the hub connection list.",
            )
            .with_alias("POST /RCH")
            .with_alias("POST /RTH"),
            OperationEntry::new(
                "mission.marker.list",
                "Map, Markers, and Zones",
                OperationKind::Query,
                TransportVariant::Extension,
                "List mission markers.",
            )
            .with_alias("GET /api/markers"),
        ])
        .expect("registry");

        let json = serde_json::to_string(&registry).expect("registry json");
        let roundtrip: OperationRegistry = serde_json::from_str(&json).expect("roundtrip");
        let resolved = roundtrip.resolve("POST /RCH").expect("alias resolution");

        assert_eq!(resolved.canonical_id.as_str(), "mission.join");
        assert_eq!(resolved.alias.as_deref(), Some("POST /RCH"));
        assert_eq!(resolved.entry.group, "Core Discovery and Session");
    }
}
