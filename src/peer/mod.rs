#[derive(Debug, Clone)]
pub struct Peer {
    dest: [u8; 16],
    last_seen: Option<f64>,
}

impl Peer {
    pub fn new(dest: [u8; 16]) -> Self {
        Self {
            dest,
            last_seen: None,
        }
    }

    pub fn mark_seen(&mut self, ts: f64) {
        self.last_seen = Some(ts);
    }

    pub fn last_seen(&self) -> Option<f64> {
        self.last_seen
    }

    pub fn dest(&self) -> [u8; 16] {
        self.dest
    }
}
