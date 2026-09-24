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
        self.spawn_inheriting_with_ifac(source, inner, worker, true)
    }

    /// Spawn a Unix shared-instance client with its parent's routing policy
    /// but without inheriting IFAC. The pinned LocalServerInterface does not
    /// configure attached LocalClientInterface objects with its IFAC keys.
    pub fn spawn_local_shared_client<T: Interface, F, R>(
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
        self.spawn_inheriting_with_ifac(source, inner, worker, false)
    }

    fn spawn_inheriting_with_ifac<T: Interface, F, R>(
        &mut self,
        source: AddressHash,
        inner: T,
        worker: F,
        inherit_ifac: bool,
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
        if !self.inherit_runtime_config_with_ifac(source, address, inherit_ifac) {
            self.stop_interface(address);
            return None;
        }
        task::spawn(worker(context));
        Some(address)
    }
}
