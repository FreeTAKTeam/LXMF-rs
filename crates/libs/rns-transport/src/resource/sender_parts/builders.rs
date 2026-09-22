impl ResourceSender {
    #[allow(dead_code)]
    pub(super) fn new_with_options_mtu(
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
        interface_mtu: usize,
    ) -> Result<Self, RnsError> {
        Self::new_with_options_mtu_and_compression(
            link,
            data,
            metadata,
            request_id,
            is_response,
            interface_mtu,
            true,
        )
    }

    pub(super) fn new_with_options_mtu_and_compression(
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
        interface_mtu: usize,
        auto_compress: bool,
    ) -> Result<Self, RnsError> {
        Self::new_segment_with_options_mtu_and_compression(
            link,
            data,
            metadata,
            request_id,
            is_response,
            interface_mtu,
            None,
            1,
            1,
            None,
            auto_compress,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)]
    pub(super) fn new_segment_with_options_mtu(
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
        interface_mtu: usize,
        original_hash: Option<Hash>,
        segment_index: u32,
        total_segments: u32,
        total_data_size: Option<u64>,
    ) -> Result<Self, RnsError> {
        Self::new_segment_with_options_mtu_and_compression(
            link,
            data,
            metadata,
            request_id,
            is_response,
            interface_mtu,
            original_hash,
            segment_index,
            total_segments,
            total_data_size,
            true,
        )
    }
}
