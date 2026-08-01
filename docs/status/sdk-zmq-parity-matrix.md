# ZeroMQ SDK access parity

<!-- GENERATED: tools/scripts/sdk_zmq_parity.py -->

SDK contract: `v2.6` in schema namespace `v2` and protocol version `2`.

Pinned-Python entries: **1811** — daemon SDK: **858**, local library: **952**, provenance-backed not applicable: **1**.

Daemon operations inventoried: **115**. Every operation uses the shared framed-RPC codec over HTTP/Unix and ZeroMQ; authorization is derived from query (`read`) versus command (`mutate`) semantics.

The complete row-level Python classification and daemon capability inventory are in [`sdk-zmq-parity.json`](sdk-zmq-parity.json). This file is generated and checked for drift in CI.

| Operation | RPC method | Auth | Capabilities | Typed contract |
|---|---|---|---|---|
| `app.attachment.associate_topic` | `sdk_attachment_associate_topic_v2` | mutate | `sdk.capability.attachments` | `Rust SDK serde types` |
| `app.attachment.delete` | `sdk_attachment_delete_v2` | mutate | `sdk.capability.attachment_delete` | `Rust SDK serde types` |
| `app.attachment.download_chunk` | `sdk_attachment_download_chunk_v2` | read | `sdk.capability.attachment_streaming` | `Rust SDK serde types` |
| `app.attachment.get` | `sdk_attachment_get_v2` | read | `sdk.capability.attachments` | `Rust SDK serde types` |
| `app.attachment.list` | `sdk_attachment_list_v2` | read | `sdk.capability.attachments` | `Rust SDK serde types` |
| `app.attachment.store` | `sdk_attachment_store_v2` | mutate | `sdk.capability.attachments` | `Rust SDK serde types` |
| `app.attachment.upload_chunk` | `sdk_attachment_upload_chunk_v2` | mutate | `sdk.capability.attachment_streaming` | `Rust SDK serde types` |
| `app.attachment.upload_commit` | `sdk_attachment_upload_commit_v2` | mutate | `sdk.capability.attachment_streaming` | `Rust SDK serde types` |
| `app.attachment.upload_start` | `sdk_attachment_upload_start_v2` | mutate | `sdk.capability.attachment_streaming` | `Rust SDK serde types` |
| `app.contact.list` | `sdk_identity_contact_list_v2` | read | `sdk.capability.contact_management` | `Rust SDK serde types` |
| `app.contact.update` | `sdk_identity_contact_update_v2` | mutate | `sdk.capability.contact_management` | `Rust SDK serde types` |
| `app.delivery.cancel` | `sdk_cancel_message_v2` | mutate | none | `docs/schemas/sdk/v2/rpc/sdk_cancel_message_v2.schema.json` |
| `app.delivery.destination_hash` | `status` | read | none | `Rust SDK serde types` |
| `app.delivery.outbound_stamp_cost` | `get_outbound_stamp_cost` | read | none | `Rust SDK serde types` |
| `app.delivery.send` | `sdk_send_v2` | mutate | none | `docs/schemas/sdk/v2/rpc/sdk_send_v2.schema.json` |
| `app.delivery.send_batch` | `sdk_send_batch_v2` | mutate | none | `docs/schemas/sdk/v2/rpc/sdk_send_batch_v2.schema.json` |
| `app.delivery.stamp_policy.get` | `stamp_policy_get` | read | none | `Rust SDK serde types` |
| `app.delivery.stamp_policy.set` | `stamp_policy_set` | mutate | none | `Rust SDK serde types` |
| `app.delivery.status` | `sdk_status_v2` | read | none | `docs/schemas/sdk/v2/rpc/sdk_status_v2.schema.json` |
| `app.delivery.ticket.generate` | `ticket_generate` | mutate | none | `Rust SDK serde types` |
| `app.delivery.trace` | `message_delivery_trace` | read | none | `Rust SDK serde types` |
| `app.event.poll` | `sdk_poll_events_v2` | read | none | `docs/schemas/sdk/v2/rpc/sdk_poll_events_v2.schema.json` |
| `app.identity.announce` | `sdk_identity_announce_now_v2` | mutate | `sdk.capability.identity_discovery` | `Rust SDK serde types` |
| `app.identity.bootstrap` | `sdk_identity_bootstrap_v2` | mutate | `sdk.capability.contact_management` | `Rust SDK serde types` |
| `app.identity.create` | `sdk_identity_create_v2` | mutate | `sdk.capability.identity_multi`, `sdk.capability.identity_import_export` | `Rust SDK serde types` |
| `app.identity.list` | `sdk_identity_list_v2` | read | `sdk.capability.identity_multi` | `Rust SDK serde types` |
| `app.identity.presence.list` | `sdk_identity_presence_list_v2` | read | `sdk.capability.identity_discovery` | `Rust SDK serde types` |
| `app.marker.create` | `sdk_marker_create_v2` | mutate | `sdk.capability.markers` | `Rust SDK serde types` |
| `app.marker.delete` | `sdk_marker_delete_v2` | mutate | `sdk.capability.markers` | `Rust SDK serde types` |
| `app.marker.list` | `sdk_marker_list_v2` | read | `sdk.capability.markers` | `Rust SDK serde types` |
| `app.marker.update_position` | `sdk_marker_update_position_v2` | mutate | `sdk.capability.markers` | `Rust SDK serde types` |
| `app.message.conversation.list` | `list_conversations` | read | none | `Rust SDK serde types` |
| `app.message.history.list` | `list_messages` | read | none | `Rust SDK serde types` |
| `app.paper.decode` | `sdk_paper_decode_v2` | mutate | `sdk.capability.paper_messages` | `Rust SDK serde types` |
| `app.paper.encode` | `sdk_paper_encode_v2` | mutate | `sdk.capability.paper_messages` | `Rust SDK serde types` |
| `app.peer.connect` | `sdk_peer_connect_v2` | mutate | `sdk.capability.peer_lifecycle` | `Rust SDK serde types` |
| `app.peer.disconnect` | `sdk_peer_disconnect_v2` | mutate | `sdk.capability.peer_lifecycle` | `Rust SDK serde types` |
| `app.peer.reconnect` | `sdk_peer_reconnect_v2` | mutate | `sdk.capability.peer_lifecycle` | `Rust SDK serde types` |
| `app.propagation.acknowledge_sync_completion` | `propagation_acknowledge_sync_completion` | mutate | none | `Rust SDK serde types` |
| `app.propagation.control.allow` | `allow_control` | mutate | none | `Rust SDK serde types` |
| `app.propagation.control.disallow` | `disallow_control` | mutate | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.allow` | `allow` | mutate | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.allow_destination` | `allow_destination` | mutate | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.auth.get` | `requires_authentication` | read | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.auth.set` | `set_authentication` | mutate | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.disallow` | `disallow` | mutate | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.disallow_destination` | `disallow_destination` | mutate | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.get` | `get_delivery_policy` | read | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.ignore` | `ignore_destination` | mutate | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.prioritise` | `prioritise` | mutate | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.prioritise_destination` | `prioritise_destination` | mutate | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.set` | `set_delivery_policy` | mutate | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.unignore` | `unignore_destination` | mutate | none | `Rust SDK serde types` |
| `app.propagation.delivery_policy.unprioritise` | `unprioritise` | mutate | none | `Rust SDK serde types` |
| `app.propagation.enable` | `propagation_enable` | mutate | none | `Rust SDK serde types` |
| `app.propagation.fetch` | `propagation_fetch` | mutate | none | `Rust SDK serde types` |
| `app.propagation.ingest` | `propagation_ingest` | mutate | none | `Rust SDK serde types` |
| `app.propagation.node.cost` | `get_outbound_propagation_cost` | read | none | `Rust SDK serde types` |
| `app.propagation.node.get` | `get_outbound_propagation_node` | read | none | `Rust SDK serde types` |
| `app.propagation.node.list` | `list_propagation_nodes` | read | none | `Rust SDK serde types` |
| `app.propagation.node.set` | `set_outbound_propagation_node` | mutate | none | `Rust SDK serde types` |
| `app.propagation.peer_maintenance` | `propagation_peer_maintenance` | mutate | none | `Rust SDK serde types` |
| `app.propagation.peer_sync` | `peer_sync` | mutate | none | `Rust SDK serde types` |
| `app.propagation.remote_download` | `propagation_remote_download` | mutate | none | `Rust SDK serde types` |
| `app.propagation.remote_fetch` | `propagation_remote_fetch` | mutate | none | `Rust SDK serde types` |
| `app.propagation.remote_status` | `propagation_remote_status` | read | none | `Rust SDK serde types` |
| `app.propagation.remote_sync` | `propagation_remote_sync` | mutate | none | `Rust SDK serde types` |
| `app.propagation.remote_unpeer` | `propagation_remote_unpeer` | mutate | none | `Rust SDK serde types` |
| `app.propagation.status` | `propagation_status` | read | none | `Rust SDK serde types` |
| `app.router.stats` | `router_stats` | read | `sdk.capability.router_management` | `Rust SDK serde types` |
| `app.router.storage_policy.get` | `router_storage_policy_get` | read | `sdk.capability.router_management` | `Rust SDK serde types` |
| `app.router.storage_policy.set` | `router_storage_policy_set` | mutate | `sdk.capability.router_management` | `Rust SDK serde types` |
| `app.runtime.cursor_hint` | `sdk_cursor_hint_v2` | read | none | `Rust SDK serde types` |
| `app.runtime.status` | `sdk_snapshot_v2` | read | none | `docs/schemas/sdk/v2/rpc/sdk_snapshot_v2.schema.json` |
| `app.telemetry.query` | `sdk_telemetry_query_v2` | read | `sdk.capability.telemetry_query` | `Rust SDK serde types` |
| `app.telemetry.subscribe` | `sdk_telemetry_subscribe_v2` | mutate | `sdk.capability.telemetry_stream` | `Rust SDK serde types` |
| `app.topic.create` | `sdk_topic_create_v2` | mutate | `sdk.capability.topics` | `Rust SDK serde types` |
| `app.topic.get` | `sdk_topic_get_v2` | read | `sdk.capability.topics` | `Rust SDK serde types` |
| `app.topic.list` | `sdk_topic_list_v2` | read | `sdk.capability.topics` | `Rust SDK serde types` |
| `app.topic.publish` | `sdk_topic_publish_v2` | mutate | `sdk.capability.topic_fanout` | `Rust SDK serde types` |
| `app.topic.subscribe` | `sdk_topic_subscribe_v2` | mutate | `sdk.capability.topic_subscriptions` | `Rust SDK serde types` |
| `app.topic.unsubscribe` | `sdk_topic_unsubscribe_v2` | mutate | `sdk.capability.topic_subscriptions` | `Rust SDK serde types` |
| `app.voice.session.close` | `sdk_voice_session_close_v2` | mutate | `sdk.capability.voice_signaling` | `Rust SDK serde types` |
| `app.voice.session.open` | `sdk_voice_session_open_v2` | mutate | `sdk.capability.voice_signaling` | `Rust SDK serde types` |
| `app.voice.session.update` | `sdk_voice_session_update_v2` | mutate | `sdk.capability.voice_signaling` | `Rust SDK serde types` |
| `app.workflow.attachment_report_publish` | `sdk_workflow_attachment_report_publish_v2` | mutate | `sdk.capability.topics`, `sdk.capability.attachments`, `sdk.capability.topic_fanout` | `Rust SDK serde types` |
| `app.workflow.mission_update_send` | `sdk_workflow_mission_update_send_v2` | mutate | `sdk.capability.contact_management`, `sdk.capability.identity_discovery`, `sdk.capability.topics`, `sdk.capability.attachments` | `Rust SDK serde types` |
| `app.workflow.peer_ready` | `sdk_workflow_peer_ready_v2` | mutate | `sdk.capability.contact_management`, `sdk.capability.identity_discovery` | `Rust SDK serde types` |
| `app.workflow.topic_sync` | `sdk_workflow_topic_sync_v2` | mutate | `sdk.capability.topics`, `sdk.capability.topic_subscriptions`, `sdk.capability.telemetry_query` | `Rust SDK serde types` |
| `rns.data_plane.announce.delivery` | `announce_delivery` | read | `sdk.capability.rns_data_plane` | `Rust SDK serde types` |
| `rns.data_plane.announce.now` | `announce_now` | mutate | `sdk.capability.rns_data_plane` | `Rust SDK serde types` |
| `rns.data_plane.announce.received` | `announce_received` | read | `sdk.capability.rns_data_plane` | `Rust SDK serde types` |
| `rns.data_plane.links.count` | `link_count` | read | `sdk.capability.rns_data_plane` | `Rust SDK serde types` |
| `rns.data_plane.packet.q` | `get_packet_q` | read | `sdk.capability.rns_data_plane` | `Rust SDK serde types` |
| `rns.data_plane.packet.rssi` | `get_packet_rssi` | read | `sdk.capability.rns_data_plane` | `Rust SDK serde types` |
| `rns.data_plane.packet.snr` | `get_packet_snr` | read | `sdk.capability.rns_data_plane` | `Rust SDK serde types` |
| `rns.interfaces.discovered` | `discovered_interfaces` | read | `sdk.capability.rns_interfaces` | `Rust SDK serde types` |
| `rns.interfaces.set` | `set_interfaces` | mutate | `sdk.capability.rns_interfaces` | `Rust SDK serde types` |
| `rns.runtime.clear.all` | `clear_all` | mutate | `sdk.capability.rns_runtime` | `Rust SDK serde types` |
| `rns.runtime.clear.messages` | `clear_messages` | mutate | `sdk.capability.rns_runtime` | `Rust SDK serde types` |
| `rns.runtime.clear.peers` | `clear_peers` | mutate | `sdk.capability.rns_runtime` | `Rust SDK serde types` |
| `rns.runtime.clear.resources` | `clear_resources` | mutate | `sdk.capability.rns_runtime` | `Rust SDK serde types` |
| `rns.runtime.status` | `daemon_status_ex` | read | `sdk.capability.rns_runtime` | `Rust SDK serde types` |
| `rns.transport.announce_queues.drop` | `drop_announce_queues` | mutate | `sdk.capability.rns_transport` | `Rust SDK serde types` |
| `rns.transport.blackholes.add` | `blackhole_identity` | mutate | `sdk.capability.rns_transport` | `Rust SDK serde types` |
| `rns.transport.blackholes.list` | `get_blackholed_identities` | read | `sdk.capability.rns_transport` | `Rust SDK serde types` |
| `rns.transport.blackholes.remove` | `unblackhole_identity` | mutate | `sdk.capability.rns_transport` | `Rust SDK serde types` |
| `rns.transport.path.drop` | `drop_path` | mutate | `sdk.capability.rns_transport` | `Rust SDK serde types` |
| `rns.transport.path.drop_all_via` | `drop_all_via` | mutate | `sdk.capability.rns_transport` | `Rust SDK serde types` |
| `rns.transport.path.first_hop_timeout` | `first_hop_timeout` | read | `sdk.capability.rns_transport` | `Rust SDK serde types` |
| `rns.transport.path.next_hop` | `next_hop` | read | `sdk.capability.rns_transport` | `Rust SDK serde types` |
| `rns.transport.path.next_hop_interface` | `next_hop_if_name` | read | `sdk.capability.rns_transport` | `Rust SDK serde types` |
| `rns.transport.path.request` | `request_path` | mutate | `sdk.capability.rns_transport` | `Rust SDK serde types` |
| `rns.transport.path.status` | `path_status` | read | `sdk.capability.rns_transport` | `Rust SDK serde types` |
| `rns.transport.rate_table` | `get_rate_table` | read | `sdk.capability.rns_transport` | `Rust SDK serde types` |
