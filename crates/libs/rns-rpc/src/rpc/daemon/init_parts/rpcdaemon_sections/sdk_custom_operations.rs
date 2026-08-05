impl RpcDaemon {
    pub fn set_sdk_custom_operations(&self, operations: Vec<SdkCustomOperationSpec>) {
        let mut guard =
            self.sdk_custom_operations.lock().expect("sdk_custom_operations mutex poisoned");
        *guard = operations
            .into_iter()
            .map(|mut operation| {
                operation.id = operation.id.trim().to_owned();
                operation.group = operation.group.trim().to_owned();
                operation.kind = operation.kind.trim().to_ascii_lowercase();
                operation.transport_variant = operation.transport_variant.trim().to_owned();
                operation.description = operation.description.trim().to_owned();
                operation.aliases = operation
                    .aliases
                    .into_iter()
                    .map(|alias| alias.trim().to_owned())
                    .filter(|alias| !alias.is_empty())
                    .collect();
                operation.required_capabilities = operation
                    .required_capabilities
                    .into_iter()
                    .map(|capability| capability.trim().to_owned())
                    .filter(|capability| !capability.is_empty())
                    .collect();
                operation
            })
            .filter(|operation| {
                !operation.id.is_empty()
                    && !operation.group.is_empty()
                    && matches!(operation.kind.as_str(), "query" | "command")
                    && !operation.transport_variant.is_empty()
            })
            .collect();
    }

    pub fn with_sdk_custom_operations(self, operations: Vec<SdkCustomOperationSpec>) -> Self {
        self.set_sdk_custom_operations(operations);
        self
    }
}
