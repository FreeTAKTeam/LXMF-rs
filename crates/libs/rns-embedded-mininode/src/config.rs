#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiniNodeConfig {
    pub app_name: &'static str,
    pub aspect: &'static str,
    pub announce_interval_ms: u64,
    pub max_neighbors: usize,
    pub max_recent_messages: usize,
    pub max_outbound_frames: usize,
    pub max_telemetry_points: usize,
    pub max_events: usize,
}

impl Default for MiniNodeConfig {
    fn default() -> Self {
        Self {
            app_name: "lxmf",
            aspect: "delivery",
            announce_interval_ms: 15 * 60 * 1000,
            max_neighbors: 16,
            max_recent_messages: 64,
            max_outbound_frames: 16,
            max_telemetry_points: 128,
            max_events: 64,
        }
    }
}
