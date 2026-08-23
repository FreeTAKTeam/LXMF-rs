// The negotiated link MTU, split out of `new.rs` to stay inside the
// repository's 500-line module budget. `include!`d into the same module.

impl Link {
    /// The MTU negotiated for this link — the smaller of what the two peers
    /// signalled, or `LEGACY_RETICULUM_MTU` if the peer signalled nothing.
    ///
    /// **This, not the local interface MTU, is what may size anything put on
    /// the wire.** The interface bounds what this node can physically carry;
    /// the negotiated value bounds what the whole path can carry, and a
    /// resource fragment sized from the former will be silently dropped by
    /// any hop that cannot take it.
    pub fn link_mtu(&self) -> usize {
        link_signalled_mtu(self.signalling)
    }

    /// Maximum cleartext carried by one encrypted Link packet.
    pub fn link_mdu(&self) -> usize {
        const IFAC_MIN_SIZE: usize = 1;
        const HEADER_MIN_SIZE: usize = 2 + 1 + ADDRESS_HASH_SIZE;
        const TOKEN_OVERHEAD: usize = 48;
        const AES_BLOCK_SIZE: usize = 16;

        let cleartext_room = self
            .link_mtu()
            .saturating_sub(IFAC_MIN_SIZE + HEADER_MIN_SIZE + TOKEN_OVERHEAD);
        (cleartext_room / AES_BLOCK_SIZE)
            .saturating_mul(AES_BLOCK_SIZE)
            .saturating_sub(1)
    }
}
