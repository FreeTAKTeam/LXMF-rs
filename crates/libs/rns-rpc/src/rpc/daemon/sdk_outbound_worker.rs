use super::*;

impl RpcDaemon {
    pub(super) fn process_outbound_delivery_command(
        bridge: &Arc<dyn OutboundBridge>,
        store: &Arc<MessagesStore>,
        delivery_traces: &Arc<Mutex<HashMap<String, Vec<DeliveryTraceEntry>>>>,
        delivery_status_lock: &Arc<Mutex<()>>,
        outbound_delivery_handoffs: &Arc<Mutex<HashSet<String>>>,
        command: OutboundDeliveryCommand,
    ) {
        let mut record = command.record;
        {
            let _status_guard =
                delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
            match store.get_message(record.id.as_str()) {
                Ok(Some(stored))
                    if stored
                        .receipt_status
                        .as_deref()
                        .is_some_and(Self::is_terminal_receipt_status) =>
                {
                    return;
                }
                Ok(Some(_)) => {}
                Ok(None) => return,
                Err(err) => {
                    log::warn!("[daemon] failed to read outbound status for {}: {err}", record.id);
                    return;
                }
            }
            outbound_delivery_handoffs
                .lock()
                .expect("outbound delivery handoffs mutex poisoned")
                .insert(record.id.clone());
        }
        record.fields = outbound_wire_fields(record.fields)
            .inspect_err(|err| log::warn!("[daemon] invalid outbound fields format: {err}"))
            .ok()
            .flatten();
        let delivery_result = bridge.deliver(&record, &command.options);
        let _status_guard =
            delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
        if let Err(err) = delivery_result {
            let status = format!("failed: {err}");
            let resolved_status = store
                .resolve_receipt_status(record.id.as_str(), status.as_str())
                .unwrap_or_else(|_| Some(status.clone()))
                .unwrap_or_else(|| status.clone());
            if resolved_status == status {
                Self::append_delivery_trace_to(delivery_traces, record.id.as_str(), status);
            }
        }
        outbound_delivery_handoffs
            .lock()
            .expect("outbound delivery handoffs mutex poisoned")
            .remove(record.id.as_str());
    }
}
