include!("dispatch_legacy_misc_parts/path_lookup.rs");
include!("dispatch_legacy_misc_parts/announce_delivery.rs");
#[path = "dispatch_legacy_misc_parts/set_display_name.rs"]
mod set_display_name;
include!("dispatch_legacy_misc_parts/blackhole_identity.rs");
include!("dispatch_legacy_misc_parts/router_management.rs");
