impl EmbeddedTransportBridge {
    pub(super) fn deliver_with_options(
        &self,
        record: &MessageRecord,
        options: OutboundDeliveryOptionsCompat,
    ) -> Result<(), std::io::Error> {
        let destination = parse_destination_hex_required(&record.destination)?;
        let peer_info =
            self.peer_crypto.lock().expect("peer map").get(&record.destination).copied();
        let peer_identity = peer_info.map(|info| info.identity);
        let (signer, source_hash) =
            resolve_signer_and_source_hash(self, &record.source, options.source_private_key)?;
        let outbound_fields = sanitize_outbound_wire_fields(record.fields.as_ref());

        let payload = build_wire_message(
            source_hash,
            destination,
            &record.title,
            &record.content,
            outbound_fields.clone(),
            &signer,
        )
        .map_err(std::io::Error::other)?;
        let opportunistic_supported = can_send_opportunistic(
            outbound_fields.as_ref(),
            opportunistic_payload(payload.as_slice(), &destination).len(),
        );
        let method_plan = DeliveryMethodPlan::from_request(
            parse_delivery_method(options.method.as_deref()),
            opportunistic_supported,
            options.try_propagation_on_fail,
        );

        let destination_hash = AddressHash::new(destination);
        let transport = self.transport.clone();
        let peer_crypto = self.peer_crypto.clone();
        let selected_propagation_node = self.selected_propagation_node.clone();
        let known_propagation_nodes = self.known_propagation_nodes.clone();
        let receipt_map = self.receipt_map.clone();
        let outbound_resource_map = self.outbound_resource_map.clone();
        let delivered_messages = self.delivered_messages.clone();
        let receipt_tx = self.receipt_tx.clone();
        let announce_targets = self.announce_targets.clone();
        let announce_last = self.last_announce_epoch_secs.clone();
        let peer_identity_cache_path = self.peer_identity_cache_path.clone();
        let message_id = record.id.clone();
        let destination_hex = record.destination.clone();
        let ticket_present =
            options.ticket.as_ref().map(|ticket| !ticket.trim().is_empty()).unwrap_or(false);
        let ticket_status =
            ticket_status(options.include_ticket, ticket_present).map(str::to_string);

        tokio::spawn(async move {
            if let Ok(mut delivered) = delivered_messages.lock() {
                delivered.remove(&message_id);
            }

            if let Some(status) = ticket_status {
                let _ = receipt_tx.send(ReceiptEvent { message_id: message_id.clone(), status });
            }

            if method_plan.downgraded_to_direct() {
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id: message_id.clone(),
                    status: "retrying: direct fallback due to opportunistic constraints"
                        .to_string(),
                });
            }

            let mut last_failure: Option<String> = None;

            if method_plan.allow_link {
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id: message_id.clone(),
                    status: "outbound_attempt: link".to_string(),
                });

                let mut identity = peer_identity;
                transport.request_path(&destination_hash, None, None).await;

                if identity.is_none() {
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
                    while tokio::time::Instant::now() < deadline {
                        if let Some(found) = transport.destination_identity(&destination_hash).await
                        {
                            identity = Some(found);
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                }

                if let Some(identity) = identity {
                    if let Ok(mut peers) = peer_crypto.lock() {
                        peers.insert(destination_hex.clone(), PeerCrypto { identity });
                    }
                    persist_peer_identity_cache(&peer_crypto, &peer_identity_cache_path);

                    let destination_desc = DestinationDesc {
                        identity,
                        address_hash: destination_hash,
                        name: DestinationName::new("lxmf", "delivery"),
                    };

                    match shared_send_via_link(
                        transport.as_ref(),
                        destination_desc,
                        payload.as_slice(),
                        Duration::from_secs(20),
                    )
                    .await
                    {
                        Ok(LinkSendResult::Packet(packet)) => {
                            let packet_hash = hex::encode(packet.hash().to_bytes());
                            track_receipt_mapping(&receipt_map, &packet_hash, &message_id);
                            trigger_rate_limited_announce(
                                &transport,
                                &announce_targets,
                                &announce_last,
                                POST_SEND_ANNOUNCE_MIN_INTERVAL_SECS,
                            );
                            let _ = receipt_tx.send(ReceiptEvent {
                                message_id,
                                status: "sent: link".to_string(),
                            });
                            return;
                        }
                        Ok(LinkSendResult::Resource(resource_hash)) => {
                            track_outbound_resource_mapping(
                                &outbound_resource_map,
                                &resource_hash,
                                &message_id,
                            );
                            trigger_rate_limited_announce(
                                &transport,
                                &announce_targets,
                                &announce_last,
                                POST_SEND_ANNOUNCE_MIN_INTERVAL_SECS,
                            );
                            let _ = receipt_tx.send(ReceiptEvent {
                                message_id,
                                status: "sending: link resource".to_string(),
                            });
                            return;
                        }
                        Err(err) => {
                            last_failure = Some(format!("failed: link {err}"));
                        }
                    }
                } else {
                    last_failure = Some("failed: peer not announced".to_string());
                }

                if !method_plan.allow_opportunistic && !method_plan.allow_propagated {
                    prune_receipt_mappings_for_message(&receipt_map, &message_id);
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id,
                        status: last_failure
                            .unwrap_or_else(|| "failed: link delivery unavailable".to_string()),
                    });
                    return;
                }
            }

            if method_plan.allow_opportunistic {
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id: message_id.clone(),
                    status: "outbound_attempt: opportunistic".to_string(),
                });
                let opportunistic_payload = opportunistic_payload(payload.as_slice(), &destination);
                let mut opportunistic_data = PacketDataBuffer::new();
                if opportunistic_data.write(opportunistic_payload).is_ok()
                    && opportunistic_supported
                {
                    let opportunistic_packet = Packet {
                        header: Header {
                            ifac_flag: IfacFlag::Open,
                            header_type: HeaderType::Type1,
                            context_flag: ContextFlag::Unset,
                            propagation_type: PropagationType::Broadcast,
                            destination_type: DestinationType::Single,
                            packet_type: PacketType::Data,
                            hops: 0,
                        },
                        ifac: None,
                        destination: destination_hash,
                        transport: None,
                        context: PacketContext::None,
                        data: opportunistic_data,
                    };
                    let opportunistic_hash = hex::encode(opportunistic_packet.hash().to_bytes());
                    track_receipt_mapping(&receipt_map, &opportunistic_hash, &message_id);
                    let opportunistic_trace =
                        transport.send_packet_with_trace(opportunistic_packet).await;
                    if !send_outcome_is_sent(opportunistic_trace.outcome) {
                        if let Ok(mut map) = receipt_map.lock() {
                            map.remove(&opportunistic_hash);
                        }
                        let failed =
                            send_outcome_status("opportunistic", opportunistic_trace.outcome);
                        last_failure = Some(failed.clone());
                        if !method_plan.allow_propagated {
                            let _ = receipt_tx.send(ReceiptEvent { message_id, status: failed });
                            return;
                        }
                    } else {
                        trigger_rate_limited_announce(
                            &transport,
                            &announce_targets,
                            &announce_last,
                            POST_SEND_ANNOUNCE_MIN_INTERVAL_SECS,
                        );
                        let status =
                            send_outcome_status("opportunistic", opportunistic_trace.outcome);
                        let _ = receipt_tx
                            .send(ReceiptEvent { message_id: message_id.clone(), status });
                        if !method_plan.allow_propagated {
                            return;
                        }

                        tokio::time::sleep(Duration::from_secs(20)).await;
                        if is_message_marked_delivered(&delivered_messages, &message_id) {
                            return;
                        }
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id: message_id.clone(),
                            status: "retrying: propagated relay after opportunistic timeout"
                                .to_string(),
                        });
                    }
                } else {
                    last_failure =
                        Some("failed: opportunistic payload too large or unsupported".to_string());
                    if !method_plan.allow_propagated {
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id,
                            status: last_failure
                                .unwrap_or_else(|| "failed: opportunistic unavailable".to_string()),
                        });
                        return;
                    }
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id: message_id.clone(),
                        status: "retrying: propagated relay after opportunistic constraints"
                            .to_string(),
                    });
                }
            }

            if !method_plan.allow_propagated {
                prune_receipt_mappings_for_message(&receipt_map, &message_id);
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id,
                    status: last_failure
                        .unwrap_or_else(|| "failed: delivery unavailable".to_string()),
                });
                return;
            }

            let mut destination_identity = peer_identity;
            if destination_identity.is_none() {
                transport.request_path(&destination_hash, None, None).await;
                let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
                while tokio::time::Instant::now() < deadline {
                    if let Some(found) = transport.destination_identity(&destination_hash).await {
                        destination_identity = Some(found);
                        if let Ok(mut peers) = peer_crypto.lock() {
                            peers.insert(destination_hex.clone(), PeerCrypto { identity: found });
                        }
                        persist_peer_identity_cache(&peer_crypto, &peer_identity_cache_path);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }

            let Some(destination_identity) = destination_identity else {
                prune_receipt_mappings_for_message(&receipt_map, &message_id);
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id,
                    status: "failed: propagated relay missing destination identity".to_string(),
                });
                return;
            };

            let propagated_payload =
                match build_propagation_envelope(payload.as_slice(), &destination_identity) {
                    Ok(encoded) => encoded,
                    Err(err) => {
                        prune_receipt_mappings_for_message(&receipt_map, &message_id);
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id,
                            status: format!("failed: propagated relay encoding error ({err})"),
                        });
                        return;
                    }
                };

            let mut relay_candidates =
                propagation_relay_candidates(&selected_propagation_node, &known_propagation_nodes)
                    .into_iter()
                    .take(MAX_ALTERNATIVE_PROPAGATION_RELAYS)
                    .collect::<Vec<_>>();
            if relay_candidates.is_empty() {
                prune_receipt_mappings_for_message(&receipt_map, &message_id);
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id,
                    status: "failed: no propagation relay selected".to_string(),
                });
                return;
            }

            let mut last_relay_failure =
                last_failure.unwrap_or_else(|| "failed: propagated relay unavailable".to_string());
            let mut attempted_relays: Vec<String> = Vec::new();
            let mut candidate_idx = 0usize;
            while candidate_idx < relay_candidates.len() {
                let relay_candidate = relay_candidates[candidate_idx].clone();
                candidate_idx += 1;
                let relay_peer = normalize_relay_destination_hash(&peer_crypto, &relay_candidate)
                    .unwrap_or(relay_candidate.clone());
                if !attempted_relays.iter().any(|entry| entry == &relay_peer) {
                    attempted_relays.push(relay_peer.clone());
                }
                let Some(relay_destination) = parse_destination_hex(&relay_peer) else {
                    last_relay_failure =
                        format!("failed: invalid propagation relay hash '{relay_peer}'");
                    continue;
                };
                let relay_hash = AddressHash::new(relay_destination);
                transport.request_path(&relay_hash, None, None).await;
                let relay_known_deadline = tokio::time::Instant::now() + Duration::from_secs(8);
                let mut relay_known = transport.destination_identity(&relay_hash).await.is_some();
                while !relay_known && tokio::time::Instant::now() < relay_known_deadline {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    relay_known = transport.destination_identity(&relay_hash).await.is_some();
                }
                if !relay_known {
                    last_relay_failure = "failed: propagation relay not announced".to_string();
                    if candidate_idx < relay_candidates.len() {
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id: message_id.clone(),
                            status: format_relay_request_status(attempted_relays.as_slice()),
                        });
                    } else if let Some(external_relay) = wait_for_external_relay_selection(
                        &selected_propagation_node,
                        &peer_crypto,
                        attempted_relays.as_slice(),
                        Duration::from_secs(8),
                    )
                    .await
                    {
                        relay_candidates.push(external_relay);
                    }
                    continue;
                }

                for attempt in 1..=2u8 {
                    if is_message_marked_delivered(&delivered_messages, &message_id) {
                        return;
                    }
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id: message_id.clone(),
                        status: format!(
                            "retrying: propagated relay attempt {attempt}/2 via {}",
                            short_hash_prefix(&relay_peer)
                        ),
                    });

                    let mut relay_data = PacketDataBuffer::new();
                    if relay_data.write(propagated_payload.as_slice()).is_err() {
                        prune_receipt_mappings_for_message(&receipt_map, &message_id);
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id,
                            status: "failed: propagated relay payload too large".to_string(),
                        });
                        return;
                    }
                    let relay_packet = Packet {
                        header: Header {
                            ifac_flag: IfacFlag::Open,
                            header_type: HeaderType::Type1,
                            context_flag: ContextFlag::Unset,
                            propagation_type: PropagationType::Broadcast,
                            destination_type: DestinationType::Single,
                            packet_type: PacketType::Data,
                            hops: 0,
                        },
                        ifac: None,
                        destination: relay_hash,
                        transport: None,
                        context: PacketContext::None,
                        data: relay_data,
                    };
                    let relay_packet_hash = hex::encode(relay_packet.hash().to_bytes());
                    track_receipt_mapping(&receipt_map, &relay_packet_hash, &message_id);
                    let relay_trace = transport.send_packet_with_trace(relay_packet).await;
                    if send_outcome_is_sent(relay_trace.outcome) {
                        trigger_rate_limited_announce(
                            &transport,
                            &announce_targets,
                            &announce_last,
                            POST_SEND_ANNOUNCE_MIN_INTERVAL_SECS,
                        );
                        if let Ok(mut selected) = selected_propagation_node.lock() {
                            *selected = Some(relay_peer.clone());
                        }
                        let _ = receipt_tx.send(ReceiptEvent {
                            message_id,
                            status: send_outcome_status("propagated relay", relay_trace.outcome),
                        });
                        return;
                    }
                    if let Ok(mut map) = receipt_map.lock() {
                        map.remove(&relay_packet_hash);
                    }
                    last_relay_failure =
                        send_outcome_status("propagated relay", relay_trace.outcome);
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }

                if candidate_idx < relay_candidates.len() {
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id: message_id.clone(),
                        status: format_relay_request_status(attempted_relays.as_slice()),
                    });
                } else if let Some(external_relay) = wait_for_external_relay_selection(
                    &selected_propagation_node,
                    &peer_crypto,
                    attempted_relays.as_slice(),
                    Duration::from_secs(8),
                )
                .await
                {
                    relay_candidates.push(external_relay);
                }
            }

            if !attempted_relays.is_empty() {
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id: message_id.clone(),
                    status: format_relay_request_status(attempted_relays.as_slice()),
                });
            }
            prune_receipt_mappings_for_message(&receipt_map, &message_id);
            let _ = receipt_tx.send(ReceiptEvent { message_id, status: last_relay_failure });
        });

        Ok(())
    }
}
