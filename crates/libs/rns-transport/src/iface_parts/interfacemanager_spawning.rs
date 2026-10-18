impl InterfaceManager {
    /// Install synchronous runtime policy before creating or polling the worker
    /// future. Failed preparation removes the new interface without starting a
    /// task; successful tasks retain the normal worker-tree lifecycle owner.
    pub fn try_spawn_as_with_mode<T: Interface, F, R, P, E>(
        &mut self,
        inner: T,
        worker: F,
        role: IfaceRole,
        mode: InterfaceMode,
        prepare: P,
    ) -> Result<AddressHash, E>
    where
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        P: FnOnce(&mut Self, AddressHash) -> Result<(), E>,
    {
        self.try_spawn_as_with_mode_and_handle(inner, worker, role, mode, prepare)
            .map(|(address, _)| address)
    }

    /// Prepared spawn that also retains the carrier's existing control handle.
    pub fn try_spawn_as_with_mode_and_handle<T: Interface, F, R, P, E>(
        &mut self,
        inner: T,
        worker: F,
        role: IfaceRole,
        mode: InterfaceMode,
        prepare: P,
    ) -> Result<(AddressHash, Arc<Mutex<T>>), E>
    where
        F: FnOnce(InterfaceContext<T>) -> R,
        R: std::future::Future<Output = ()> + Send + 'static,
        P: FnOnce(&mut Self, AddressHash) -> Result<(), E>,
    {
        let context = self.new_context_with_role_and_mode(inner, role, mode);
        let address = *context.channel.address();
        if let Err(error) = prepare(self, address) {
            self.stop_interface(address);
            return Err(error);
        }
        let handle = context.inner.clone();
        self.worker_tasks.insert(address, task::spawn(worker(context)));
        Ok((address, handle))
    }

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
        self.worker_tasks.insert(address, task::spawn(worker(context)));
        Some(address)
    }
}
