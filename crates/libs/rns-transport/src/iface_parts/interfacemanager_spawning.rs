impl InterfaceManager {
    /// Spawn an accepted child only after copying its parent's runtime policy.
    /// This ensures the worker's first poll cannot observe default IFAC state.
    pub fn spawn_inheriting<T: Interface, F, R>(
        &mut self,
        source: AddressHash,
        inner: T,
        worker: F,
    ) -> Option<AddressHash>
    where
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        R::Output: Send + 'static,
    {
        let context = self.new_context_with_role_and_mode(
            inner,
            IfaceRole::default(),
            InterfaceMode::default(),
        );
        let address = *context.channel.address();
        if !self.inherit_runtime_config(source, address) {
            self.stop_interface(address);
            return None;
        }
        task::spawn(worker(context));
        Some(address)
    }
}
