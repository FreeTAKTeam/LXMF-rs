pub const FIELD_EMBEDDED_LXMS: u8 = 0x01;
pub const FIELD_TELEMETRY: u8 = 0x02;
pub const FIELD_TELEMETRY_STREAM: u8 = 0x03;
pub const FIELD_ICON_APPEARANCE: u8 = 0x04;
pub const FIELD_FILE_ATTACHMENTS: u8 = 0x05;
pub const FIELD_IMAGE: u8 = 0x06;
pub const FIELD_AUDIO: u8 = 0x07;
pub const FIELD_THREAD: u8 = 0x08;
pub const FIELD_COMMANDS: u8 = 0x09;
pub const FIELD_RESULTS: u8 = 0x0A;
pub const FIELD_GROUP: u8 = 0x0B;
pub const FIELD_TICKET: u8 = 0x0C;
pub const FIELD_EVENT: u8 = 0x0D;
pub const FIELD_RNR_REFS: u8 = 0x0E;
pub const FIELD_RENDERER: u8 = 0x0F;
pub const FIELD_COLUMBA_META: u8 = 0x70;
pub const FIELD_CUSTOM_TYPE: u8 = 0xFB;
pub const FIELD_CUSTOM_DATA: u8 = 0xFC;
pub const FIELD_CUSTOM_META: u8 = 0xFD;
pub const FIELD_NON_SPECIFIC: u8 = 0xFE;
pub const FIELD_DEBUG: u8 = 0xFF;

pub const RENDERER_PLAIN: u8 = 0x00;
pub const RENDERER_MICRON: u8 = 0x01;
pub const RENDERER_MARKDOWN: u8 = 0x02;
pub const RENDERER_BBCODE: u8 = 0x03;

pub const AM_CODEC2_450PWB: u8 = 0x01;
pub const AM_CODEC2_450: u8 = 0x02;
pub const AM_CODEC2_700C: u8 = 0x03;
pub const AM_CODEC2_1200: u8 = 0x04;
pub const AM_CODEC2_1300: u8 = 0x05;
pub const AM_CODEC2_1400: u8 = 0x06;
pub const AM_CODEC2_1600: u8 = 0x07;
pub const AM_CODEC2_2400: u8 = 0x08;
pub const AM_CODEC2_3200: u8 = 0x09;
pub const AM_OPUS_OGG: u8 = 0x10;
pub const AM_OPUS_LBW: u8 = 0x11;
pub const AM_OPUS_MBW: u8 = 0x12;
pub const AM_OPUS_PTT: u8 = 0x13;
pub const AM_OPUS_RT_HDX: u8 = 0x14;
pub const AM_OPUS_RT_FDX: u8 = 0x15;
pub const AM_OPUS_STANDARD: u8 = 0x16;
pub const AM_OPUS_HQ: u8 = 0x17;
pub const AM_OPUS_BROADCAST: u8 = 0x18;
pub const AM_OPUS_LOSSLESS: u8 = 0x19;
pub const AM_CUSTOM: u8 = 0xFF;

pub const WORKBLOCK_EXPAND_ROUNDS: usize = 3000;
pub const WORKBLOCK_EXPAND_ROUNDS_PN: usize = 1000;
pub const DESTINATION_LENGTH: usize = 16;
pub const SIGNATURE_LENGTH: usize = 64;
pub const TICKET_LENGTH: usize = 16;
pub const TIMESTAMP_SIZE: usize = 8;
pub const STRUCT_OVERHEAD: usize = 8;
pub const LXMF_OVERHEAD: usize =
    (2 * DESTINATION_LENGTH) + SIGNATURE_LENGTH + TIMESTAMP_SIZE + STRUCT_OVERHEAD;

pub const RETICULUM_MTU: usize = 500;
pub const RETICULUM_TRUNCATED_HASH_LENGTH_BYTES: usize = 16;
pub const RETICULUM_HEADER_MINSIZE: usize = 2 + 1 + RETICULUM_TRUNCATED_HASH_LENGTH_BYTES;
pub const RETICULUM_HEADER_MAXSIZE: usize = 2 + 1 + (RETICULUM_TRUNCATED_HASH_LENGTH_BYTES * 2);
pub const RETICULUM_IFAC_MIN_SIZE: usize = 1;
pub const RETICULUM_MDU: usize = RETICULUM_MTU - RETICULUM_HEADER_MAXSIZE - RETICULUM_IFAC_MIN_SIZE;
pub const RETICULUM_TOKEN_OVERHEAD: usize = 48;
pub const RETICULUM_AES_BLOCKSIZE: usize = 16;
pub const RETICULUM_KEYSIZE_DIV_16: usize = 32;

pub const ENCRYPTED_MDU: usize =
    ((RETICULUM_MDU - RETICULUM_TOKEN_OVERHEAD - RETICULUM_KEYSIZE_DIV_16)
        / RETICULUM_AES_BLOCKSIZE)
        * RETICULUM_AES_BLOCKSIZE
        - 1;
pub const PLAIN_MDU: usize = RETICULUM_MDU;
pub const LINK_PACKET_MDU: usize = ((RETICULUM_MTU
    - RETICULUM_IFAC_MIN_SIZE
    - RETICULUM_HEADER_MINSIZE
    - RETICULUM_TOKEN_OVERHEAD)
    / RETICULUM_AES_BLOCKSIZE)
    * RETICULUM_AES_BLOCKSIZE
    - 1;

pub const ENCRYPTED_PACKET_MDU: usize = ENCRYPTED_MDU + TIMESTAMP_SIZE;
pub const ENCRYPTED_PACKET_MAX_CONTENT: usize =
    ENCRYPTED_PACKET_MDU - LXMF_OVERHEAD + DESTINATION_LENGTH;
pub const LINK_PACKET_MAX_CONTENT: usize = LINK_PACKET_MDU - LXMF_OVERHEAD;
pub const PLAIN_PACKET_MAX_CONTENT: usize = PLAIN_MDU - LXMF_OVERHEAD + DESTINATION_LENGTH;

pub const QR_MAX_STORAGE: usize = 2953;
pub const URI_SCHEMA_LENGTH: usize = 3;
pub const PAPER_MDU: usize = ((QR_MAX_STORAGE - (URI_SCHEMA_LENGTH + 3)) * 6) / 8;

pub const PN_META_NAME: u8 = 0x01;
pub const PN_META_VERSION: u8 = 0x00;
pub const PN_META_SYNC_STRATUM: u8 = 0x02;
pub const PN_META_SYNC_THROTTLE: u8 = 0x03;
pub const PN_META_AUTH_BAND: u8 = 0x04;
pub const PN_META_UTIL_PRESSURE: u8 = 0x05;
pub const PN_META_CUSTOM: u8 = 0xFF;

pub const PROPAGATION_COST_MIN: u32 = 13;
pub const PROPAGATION_COST_FLEX: u32 = 3;
pub const PROPAGATION_COST: u32 = 16;
pub const PROPAGATION_LIMIT: u32 = 256;
pub const SYNC_LIMIT: u32 = PROPAGATION_LIMIT * 40;
pub const PEERING_COST: u32 = 18;
pub const MAX_PEERING_COST: u32 = 26;
