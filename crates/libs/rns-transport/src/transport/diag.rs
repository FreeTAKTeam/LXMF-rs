use std::sync::OnceLock;

use crate::hash::AddressHash;

use super::path_table::PathTable;
use super::{SendPacketOutcome, TxDispatchTrace};

pub(super) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("RETICULUMD_DIAGNOSTICS")
            .or_else(|_| std::env::var("RETICULUM_TRANSPORT_DIAGNOSTICS"))
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on" | "debug"
                )
            })
            .unwrap_or(false)
    })
}

pub(super) fn log_route_lookup(path_table: &PathTable, destination: &AddressHash) {
    if !enabled() {
        return;
    }

    if let Some(entry) = path_table.get(destination) {
        log::trace!(
            "[tp-diag] route_lookup dst={} hops={} via_next_hop={} via_iface={}",
            destination,
            entry.hops,
            entry.received_from,
            entry.iface
        );
        log::info!(
            "[tp-diag] route_lookup dst={} hops={} via_next_hop={} via_iface={}",
            destination,
            entry.hops,
            entry.received_from,
            entry.iface
        );
    } else {
        log::trace!("[tp-diag] route_lookup dst={} missing", destination);
        log::info!("[tp-diag] route_lookup dst={} missing", destination);
    }
}

pub(super) fn log_direct_send(
    iface: AddressHash,
    outcome: SendPacketOutcome,
    dispatch: &TxDispatchTrace,
) {
    if !enabled() {
        return;
    }

    log::trace!(
        "[tp-diag] direct_send iface={} outcome={:?} matched={} sent={} failed={}",
        iface,
        outcome,
        dispatch.matched_ifaces,
        dispatch.sent_ifaces,
        dispatch.failed_ifaces
    );
    log::info!(
        "[tp-diag] direct_send iface={} outcome={:?} matched={} sent={} failed={}",
        iface,
        outcome,
        dispatch.matched_ifaces,
        dispatch.sent_ifaces,
        dispatch.failed_ifaces
    );
}

pub(super) fn log_broadcast_send(outcome: SendPacketOutcome, dispatch: &TxDispatchTrace) {
    if !enabled() {
        return;
    }

    log::trace!(
        "[tp-diag] broadcast_send outcome={:?} matched={} sent={} failed={}",
        outcome,
        dispatch.matched_ifaces,
        dispatch.sent_ifaces,
        dispatch.failed_ifaces
    );
    log::info!(
        "[tp-diag] broadcast_send outcome={:?} matched={} sent={} failed={}",
        outcome,
        dispatch.matched_ifaces,
        dispatch.sent_ifaces,
        dispatch.failed_ifaces
    );
}
