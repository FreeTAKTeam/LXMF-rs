#[derive(Clone)]
pub struct LinkPayload {
    buffer: [u8; PACKET_MDU],
    len: usize,
    context: PacketContext,
    request_id: Option<[u8; ADDRESS_HASH_SIZE]>,
}

impl LinkPayload {
    pub fn new() -> Self {
        Self { buffer: [0u8; PACKET_MDU], len: 0, context: PacketContext::None, request_id: None }
    }

    pub fn new_from_slice(data: &[u8]) -> Self {
        Self::new_from_slice_with_context(data, PacketContext::None)
    }

    pub fn new_from_slice_with_context(data: &[u8], context: PacketContext) -> Self {
        let mut buffer = [0u8; PACKET_MDU];
        let len = min(data.len(), buffer.len());
        buffer[..len].copy_from_slice(&data[..len]);

        Self { buffer, len, context, request_id: None }
    }

    pub fn new_from_slice_with_context_and_request_id(
        data: &[u8],
        context: PacketContext,
        request_id: Option<[u8; ADDRESS_HASH_SIZE]>,
    ) -> Self {
        let mut payload = Self::new_from_slice_with_context(data, context);
        payload.request_id = request_id;
        payload
    }

    pub fn new_from_vec(data: &[u8]) -> Self {
        let mut buffer = [0u8; PACKET_MDU];
        let copy_len = min(buffer.len(), data.len());
        buffer[..copy_len].copy_from_slice(&data[..copy_len]);

        Self { buffer, len: data.len(), context: PacketContext::None, request_id: None }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn context(&self) -> PacketContext {
        self.context
    }

    pub fn request_id(&self) -> Option<[u8; ADDRESS_HASH_SIZE]> {
        self.request_id
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buffer[..self.len]
    }
}

impl Default for LinkPayload {
    fn default() -> Self {
        Self::new()
    }
}
