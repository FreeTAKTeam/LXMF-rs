use crate::error::LxmfError;
use crate::message::WireMessage;

/// Pluggable outbound transport surface used by the router.
pub trait TransportPlugin: Send + Sync {
    /// Stable plugin identifier for logs/debug output.
    fn name(&self) -> &str;

    /// Whether outbound delivery is currently configured and available.
    fn has_outbound_sender(&self) -> bool;

    /// Send one outbound wire message to the underlying transport.
    fn send_outbound(&self, message: &WireMessage) -> Result<(), LxmfError>;
}
