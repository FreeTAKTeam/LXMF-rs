use rns_bluetooth::{DeviceId, OwnerId, SessionRegistry};

fn main() -> rns_bluetooth::Result<()> {
    rns_bluetooth::android::require_bridge_version(1)?;
    let registry = SessionRegistry::new(2)?;
    let radio = registry.reserve(OwnerId("radio".into()), DeviceId("synthetic-radio".into()))?;
    let plugin = registry.reserve(OwnerId("plugin".into()), DeviceId("synthetic-plugin".into()))?;
    registry.release(&plugin)?;
    assert_eq!(registry.device(&radio)?, DeviceId("synthetic-radio".into()));
    registry.release(&radio)?;
    Ok(())
}
