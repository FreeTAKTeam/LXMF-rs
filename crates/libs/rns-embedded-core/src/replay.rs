#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ReplayWindow {
    highest: Option<u64>,
    seen: u64,
}

impl ReplayWindow {
    pub fn new() -> Self {
        Self { highest: None, seen: 0 }
    }

    pub fn highest(&self) -> Option<u64> {
        self.highest
    }

    pub fn accept(&mut self, sequence: u64) -> bool {
        match self.highest {
            None => {
                self.highest = Some(sequence);
                self.seen = 1;
                true
            }
            Some(highest) if sequence > highest => {
                let shift = sequence.saturating_sub(highest);
                self.seen = if shift >= 64 { 0 } else { self.seen << (shift as u32) };
                self.highest = Some(sequence);
                self.seen |= 1;
                true
            }
            Some(highest) => {
                let delta = highest - sequence;
                if delta >= 64 {
                    return false;
                }
                let bit = 1_u64 << (delta as u32);
                if (self.seen & bit) != 0 {
                    return false;
                }
                self.seen |= bit;
                true
            }
        }
    }
}

impl Default for ReplayWindow {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::ReplayWindow;

    #[test]
    fn accepts_forward_and_rejects_duplicates() {
        let mut window = ReplayWindow::new();
        assert!(window.accept(10));
        assert!(window.accept(11));
        assert!(!window.accept(11));
        assert!(window.accept(9));
        assert!(!window.accept(9));
    }

    #[test]
    fn rejects_far_old_sequences() {
        let mut window = ReplayWindow::new();
        assert!(window.accept(200));
        assert!(!window.accept(100));
    }
}
