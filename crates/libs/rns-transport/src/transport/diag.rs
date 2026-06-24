use crate::hash::AddressHash;

use super::path_table::PathTable;
use super::{SendPacketOutcome, TxDispatchTrace};

pub(super) fn log_route_lookup(path_table: &PathTable, destination: &AddressHash) {
    if let Some(entry) = path_table.get(destination) {
        log::trace!(
            "[tp-diag] route_lookup dst={} hops={} via_next_hop={} via_iface={}",
            destination,
            entry.hops,
            entry.received_from,
            entry.iface
        );
    } else {
        log::trace!("[tp-diag] route_lookup dst={} missing", destination);
    }
}

pub(super) fn log_direct_send(
    iface: AddressHash,
    outcome: SendPacketOutcome,
    dispatch: &TxDispatchTrace,
) {
    log::trace!(
        "[tp-diag] direct_send iface={} outcome={:?} matched={} sent={} failed={}",
        iface,
        outcome,
        dispatch.matched_ifaces,
        dispatch.sent_ifaces,
        dispatch.failed_ifaces
    );
}

pub(super) fn log_broadcast_send(outcome: SendPacketOutcome, dispatch: &TxDispatchTrace) {
    log::trace!(
        "[tp-diag] broadcast_send outcome={:?} matched={} sent={} failed={}",
        outcome,
        dispatch.matched_ifaces,
        dispatch.sent_ifaces,
        dispatch.failed_ifaces
    );
}
