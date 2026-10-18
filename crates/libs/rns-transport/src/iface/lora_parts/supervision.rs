impl LoraInterface {
    pub async fn spawn(context: InterfaceContext<LoraInterface>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let ifac_state = context.channel.ifac_state.clone();
        let ifac_violations = context.channel.ifac_violations.clone();
        let (
            endpoint,
            config,
            flow_control,
            id_beacon,
            reconnect_backoff,
            max_reconnect_backoff,
            startup_response_timeout,
            management_frame_rx,
        ) = {
            let guard = context.inner.lock().expect("lora interface mutex poisoned");
            (
                guard.endpoint.clone(),
                guard.config,
                guard.flow_control,
                guard.id_beacon.clone(),
                guard.reconnect_backoff,
                guard.max_reconnect_backoff,
                guard.startup_response_timeout,
                guard.management_frame_rx.clone(),
            )
        };

        let online = context.channel.online.clone();
        online.store(false, std::sync::atomic::Ordering::Release);
        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));
        let mut active_backoff = reconnect_backoff;

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            match &endpoint {
                LoraEndpoint::Serial { device, baud_rate } => {
                    let port = match tokio_serial::new(device.clone(), *baud_rate)
                        .data_bits(DataBits::Eight)
                        .parity(Parity::None)
                        .stop_bits(StopBits::One)
                        .flow_control(FlowControl::None)
                        .open_native_async()
                    {
                        Ok(port) => port,
                        Err(err) => {
                            log::warn!(
                                "failed to open LoRa serial device={} baud_rate={} err={}",
                                device,
                                baud_rate,
                                err
                            );
                            tokio::time::sleep(active_backoff).await;
                            active_backoff =
                                bounded_backoff_next(active_backoff, max_reconnect_backoff);
                            continue;
                        }
                    };

                    log::info!(
                        "opened LoRa serial device={} baud_rate={} iface={} frequency_hz={} bandwidth_hz={} sf={} cr={}",
                        device,
                        baud_rate,
                        iface_address,
                        config.frequency_hz,
                        config.bandwidth_hz,
                        config.spreading_factor,
                        config.coding_rate
                    );
                    online.store(true, std::sync::atomic::Ordering::Release);
                    active_backoff = reconnect_backoff;
                    run_lora_kiss_stream(
                        port,
                        LoraStreamRun {
                            interface: context.inner.clone(),
                            cancel: context.cancel.clone(),
                            iface_address,
                            endpoint_label: device.clone(),
                            config,
                            flow_control,
                            id_beacon: id_beacon.clone(),
                            activity_probe: None,
                            startup_response_timeout,
                            management_frame_rx: management_frame_rx.clone(),
                            rx_channel: rx_channel.clone(),
                            tx_channel: tx_channel.clone(),
                            ifac_state: ifac_state.clone(),
                            ifac_violations: ifac_violations.clone(),
                        },
                    )
                    .await;
                    online.store(false, std::sync::atomic::Ordering::Release);
                    mark_lora_link_offline(&context.inner);
                }
                LoraEndpoint::Tcp { addr } => {
                    let stream = match TcpStream::connect(addr.clone()).await {
                        Ok(stream) => stream,
                        Err(err) => {
                            log::warn!("failed to connect LoRa tcp addr={} err={}", addr, err);
                            tokio::time::sleep(active_backoff).await;
                            active_backoff =
                                bounded_backoff_next(active_backoff, max_reconnect_backoff);
                            continue;
                        }
                    };

                    log::info!(
                        "opened LoRa tcp addr={} iface={} frequency_hz={} bandwidth_hz={} sf={} cr={}",
                        addr,
                        iface_address,
                        config.frequency_hz,
                        config.bandwidth_hz,
                        config.spreading_factor,
                        config.coding_rate
                    );
                    online.store(true, std::sync::atomic::Ordering::Release);
                    active_backoff = reconnect_backoff;
                    run_lora_kiss_stream(
                        stream,
                        LoraStreamRun {
                            interface: context.inner.clone(),
                            cancel: context.cancel.clone(),
                            iface_address,
                            endpoint_label: addr.clone(),
                            config,
                            flow_control,
                            id_beacon: id_beacon.clone(),
                            activity_probe: Some(rnode_tcp_activity_probe()),
                            startup_response_timeout,
                            management_frame_rx: management_frame_rx.clone(),
                            rx_channel: rx_channel.clone(),
                            tx_channel: tx_channel.clone(),
                            ifac_state: ifac_state.clone(),
                            ifac_violations: ifac_violations.clone(),
                        },
                    )
                    .await;
                    online.store(false, std::sync::atomic::Ordering::Release);
                    mark_lora_link_offline(&context.inner);
                }
            };

            if context.cancel.is_cancelled() {
                break;
            }
            tokio::time::sleep(active_backoff).await;
            active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
        }

        online.store(false, std::sync::atomic::Ordering::Release);
        mark_lora_link_offline(&context.inner);
        iface_stop.cancel();
    }
}

fn mark_lora_link_offline(inner: &Arc<std::sync::Mutex<LoraInterface>>) {
    inner.lock().expect("lora interface mutex poisoned").mark_link_offline();
}
