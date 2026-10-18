impl InterfaceStartupBatch {
    pub(crate) fn into_owned_roots(
        self,
        records: &[InterfaceRecord],
    ) -> Vec<crate::configured_interface_runtime::ConfiguredInterfaceStartup> {
        use crate::configured_interface_runtime::{
            ConfiguredInterfaceStartup, NativeInterfaceRuntime, NativeRuntimeStatus as Status,
        };
        let mut native = self
            .configured_roots
            .into_iter()
            .map(|(index, address)| (index, NativeInterfaceRuntime::new(address, None)))
            .collect::<Vec<_>>();
        macro_rules! statuses {
            ($refreshes:expr, $variant:ident) => {
                for refresh in $refreshes {
                    let root = retained_native_root(&mut native, refresh.runtime_iface);

                    root.status = Some(Status::$variant(refresh.status));
                }
            };
        }
        statuses!(self.auto_runtime_refreshes, Auto);
        statuses!(self.pipe_runtime_refreshes, Pipe);
        statuses!(self.udp_runtime_refreshes, Udp);
        statuses!(self.serial_runtime_refreshes, Serial);
        statuses!(self.ble_gatt_runtime_refreshes, BleGatt);
        statuses!(self.reticulum_ble_runtime_refreshes, ReticulumBle);
        statuses!(self.i2p_runtime_refreshes, I2p);
        statuses!(self.weave_runtime_refreshes, Weave);
        statuses!(self.rnode_multi_runtime_refreshes, RNodeMulti);
        statuses!(self.lora_runtime_refreshes, Lora);
        #[cfg(feature = "vrn76-kiss-ble")]
        statuses!(self.vrn76_runtime_refreshes, Vrn76);
        for refresh in self.kiss_runtime_refreshes {
            let root = retained_native_root(&mut native, refresh.runtime_iface);

            root.status = Some(Status::Kiss { key: refresh.runtime_key, status: refresh.status });
        }
        for refresh in self.tcp_runtime_refreshes {
            let root = retained_native_root(&mut native, refresh.runtime_iface);

            root.status = Some(match refresh.status {
                TcpRuntimeStatusSource::Stream(status) => Status::TcpClient(status),
                TcpRuntimeStatusSource::Listener(status) => Status::TcpListener(status),
            });
        }
        for refresh in self.meshtastic_runtime_refreshes {
            let root = retained_native_root(&mut native, refresh.runtime_iface);

            root.status = Some(Status::Meshtastic(refresh.status));
            root.auxiliary.push(refresh.simulation);
        }
        for shutdown in self.auto_runtime_shutdowns {
            let root = retained_native_root(&mut native, shutdown.host_iface);

            root.auto_shutdown = Some(shutdown);
        }
        for binding in self.rnode_management_bindings {
            let root = retained_native_root(&mut native, binding.runtime_iface);

            root.rnode.push((binding.name, binding.handle));
        }
        for binding in self.weave_control_bindings {
            let root = retained_native_root(&mut native, binding.runtime_iface);

            root.weave.push((binding.name, binding.handle));
        }
        let entries = records
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, record)| {
                let position = native.iter().position(|(recipe, _)| *recipe == index);
                ConfiguredInterfaceStartup {
                    index,
                    record,
                    native: position.map(|position| native.swap_remove(position).1),
                }
            })
            .collect();
        assert!(native.is_empty(), "every native root retains its original configured record");
        entries
    }
}

fn retained_native_root(
    roots: &mut [(usize, crate::configured_interface_runtime::NativeInterfaceRuntime)],
    address: AddressHash,
) -> &mut crate::configured_interface_runtime::NativeInterfaceRuntime {
    &mut roots
        .iter_mut()
        .find(|(_, root)| root.address == address)
        .expect("every startup status and effect belongs to a retained native root")
        .1
}
