//! Capture a failed SDK submission without preparing or transmitting a second message.
use std::cell::RefCell;

use rns_transport::destination::DestinationDesc;

pub struct CapturedOutgoing {
    pub(crate) wire: Vec<u8>,
    pub(crate) remote: DestinationDesc,
}

thread_local! {
    static CAPTURE: RefCell<Option<Option<CapturedOutgoing>>> = const { RefCell::new(None) };
}

/// Run the synchronous SDK send on its existing blocking thread. The capture is
/// scoped to this submission and contains no custody or delivery decisions.
pub fn capture_outgoing<T>(send: impl FnOnce() -> T) -> (T, Option<CapturedOutgoing>) {
    struct Restore(Option<Option<CapturedOutgoing>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CAPTURE.with(|capture| *capture.borrow_mut() = self.0.take());
        }
    }
    let previous = CAPTURE.with(|capture| capture.replace(Some(None)));
    let _restore = Restore(previous);
    let result = send();
    let outgoing = CAPTURE.with(|capture| capture.borrow_mut().as_mut().and_then(Option::take));
    (result, outgoing)
}

pub(crate) fn record(wire: &[u8], remote: DestinationDesc) {
    CAPTURE.with(|capture| {
        if let Some(slot) = capture.borrow_mut().as_mut() {
            *slot = Some(CapturedOutgoing { wire: wire.to_vec(), remote });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;
    use rns_transport::{
        destination::{DestinationName, SingleOutputDestination},
        identity::PrivateIdentity,
    };

    #[test]
    fn capture_is_scoped_and_preserves_original_wire() {
        let peer = PrivateIdentity::new_from_rand(OsRng);
        let remote = SingleOutputDestination::new(
            *peer.as_identity(),
            DestinationName::new("lxmf", "delivery"),
        )
        .desc;
        record(&[1], remote);
        let (result, outgoing) = capture_outgoing(|| {
            record(&[2, 3], remote);
            let (_, nested) = capture_outgoing(|| record(&[4], remote));
            assert_eq!(nested.unwrap().wire, [4]);
            7
        });
        assert_eq!(result, 7);
        assert_eq!(outgoing.unwrap().wire, [2, 3]);
        assert!(capture_outgoing(|| ()).1.is_none());
    }
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capture_survives_block_on_await_and_is_thread_local() {
    use rand_core::OsRng;
    use rns_transport::{
        destination::{DestinationName, SingleOutputDestination},
        identity::PrivateIdentity,
    };
    let remote = SingleOutputDestination::new(
        *PrivateIdentity::new_from_rand(OsRng).as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )
    .desc;
    let handle = tokio::runtime::Handle::current();
    let first = tokio::task::spawn_blocking(move || {
        capture_outgoing(|| {
            handle.block_on(async {
                tokio::task::yield_now().await;
                record(&[1], remote);
            })
        })
    });
    let second = tokio::task::spawn_blocking(move || capture_outgoing(|| record(&[2], remote)));
    assert_eq!(first.await.unwrap().1.unwrap().wire, [1]);
    assert_eq!(second.await.unwrap().1.unwrap().wire, [2]);
}
