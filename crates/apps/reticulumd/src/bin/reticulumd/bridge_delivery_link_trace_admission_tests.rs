use super::*;

impl Harness {
    pub(super) async fn admit(&mut self, case: &Value, index: usize, existing_id: Option<String>) {
        let destination_identity = identity(if !self.propagated || index == 0 {
            0x22
        } else if index == 1 {
            0x33
        } else {
            0x44
        });
        let destination = SingleInputDestination::new(
            destination_identity.clone(),
            DestinationName::new("lxmf", "delivery"),
        )
        .desc
        .address_hash;
        let id = existing_id.unwrap_or_else(|| {
            let reply = self
                .admissions
                .broker_command(BrokerCommand::Admit {
                    owner: "test-owner".into(),
                    source: hex::encode([1; 16]),
                    request: rns_rpc::broker::AdmitRequest {
                        identity: "test-source".into(),
                        operation_id: format!("trace-{index}"),
                        destination: destination.to_hex_string(),
                        title: String::new(),
                        content: format!("trace message {index}"),
                        fields: None,
                        options: rns_rpc::OutboundDeliveryOptions::default(),
                    },
                })
                .expect("real durable admission");
            let id = reply["message_id"].as_str().expect("id").to_string();
            assert_eq!(
                self.admissions
                    .broker_command(BrokerCommand::DispatchState {
                        message_id: id.clone(),
                        state: "scheduled".into()
                    })
                    .expect("scheduled")["claimed"],
                true
            );
            id
        });
        let mut task = delivery_task_for_propagation_cost_lookup(self.daemon.clone());
        task.message_id = id.clone();
        task.destination = destination.as_slice().try_into().expect("destination");
        task.destination_hash = destination;
        task.destination_hex = destination.to_hex_string();
        task.requested_method = self.method;
        task.peer_identity = Some(*destination_identity.as_identity());
        task.stamp_cost = Some(0);
        task.transport = self.wire.sender.clone();
        task.receipt_tx = self.publisher.clone();
        task.receipt_map = self.receipt_map.clone();
        task.outbound_resource_map = self.resources.clone();
        task.outbound_propagation_link = self.propagation_link.clone();
        if self.propagated {
            task.propagation_node_hex = Some(self.wire.descriptor.address_hash.to_hex_string());
            task.propagation_node_identity = Some(self.wire.descriptor.identity);
        }
        let payload =
            hex::decode(case["signed_wire_hex"][index].as_str().expect("prepared reference wire"))
                .expect("wire bytes");
        let mut prepared = crate::bridge::delivery_task::PreparedDeliveryPayload {
            lxmf_payload: payload,
            propagation: None,
        };
        if self.propagated {
            // The oracle explicitly disables deferred stamp production. Preserve
            // its unstamped envelope shape; stamp acceptance has separate coverage.
            let wire = lxmf::WireMessage::unpack(&prepared.lxmf_payload).expect("signed LXMF");
            let core = rns_core::identity::Identity::new_from_slices(
                destination_identity.as_identity().public_key_bytes(),
                destination_identity.as_identity().verifying_key_bytes(),
            );
            let (data, transient_id) = wire
                .pack_propagation_transient_with_rng(&core, OsRng)
                .expect("real propagation encryption");
            let bytes = lxmf::WireMessage::pack_propagation_envelope(1773999000.25, &data, None)
                .expect("unstamped oracle envelope");
            prepared.propagation = Some(PreparedPropagationPayload {
                propagation_node_hex: self.wire.descriptor.address_hash.to_hex_string(),
                propagation_hash: self.wire.descriptor.address_hash,
                target_cost: 0,
                payload: crate::bridge::propagation::PropagationPayload {
                    bytes,
                    transient_id,
                    stamp_value: 0,
                },
            });
        }
        let expected = prepared
            .propagation
            .as_ref()
            .map_or(&prepared.lxmf_payload, |p| &p.payload.bytes)
            .clone();
        assert_eq!(
            expected.len() as u64,
            case["prepared_transfer_bytes"][index].as_u64().expect("reference length")
        );
        self.daemon
            .persist_durable_prepared_payload(&id, encode_prepared(&prepared))
            .expect("persist exact prepared bytes");
        self.publisher.reserve_completion(&id, self.propagated).expect("receipt reservations");
        let delivery = admitted(&self.scheduler, task);
        let wakeup = delivery.task.wakeup.clone();
        let control = Arc::new(TraceControl::default());
        *control.measurements.lock().expect("measurements") = (
            case["medium_timeout"].as_f64().expect("medium"),
            case["lowest_bitrate"].as_u64().expect("bitrate"),
            case["first_hop"].as_f64().expect("first hop"),
        );
        *control.fault.lock().expect("fault") = match case["send_effect"].as_str().expect("effect")
        {
            "receipt" => ProducerFault::None,
            "none" => ProducerFault::NoReceipt,
            "raise" => ProducerFault::Error,
            input => panic!("unsupported producer {input}"),
        };
        *wakeup.trace.lock().expect("attach control") = Some(control.clone());
        let peer = destination.to_hex_string();
        let owner = tokio::spawn(completion::complete_delivery(
            delivery,
            self.global.clone(),
            Arc::new(Semaphore::new(1)),
            self.stamp.clone(),
            self.scheduler.metrics.clone(),
            peer,
            self.stop.subscribe(),
        ));
        self.messages.push(MessageOwner {
            id,
            control,
            wakeup,
            owner,
            admitted_at: Instant::now(),
            expected,
        });
    }
}
