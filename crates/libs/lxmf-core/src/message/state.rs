#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Generating,
    Outbound,
    Sending,
    Sent,
    Delivered,
    Rejected,
    Cancelled,
    Failed,
}
