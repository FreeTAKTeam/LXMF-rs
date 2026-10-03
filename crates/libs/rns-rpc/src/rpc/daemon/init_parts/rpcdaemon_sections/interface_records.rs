impl RpcDaemon {
    pub fn interface_records(&self) -> Vec<InterfaceRecord> {
        self.interfaces.lock().expect("interfaces mutex poisoned").clone()
    }
}
