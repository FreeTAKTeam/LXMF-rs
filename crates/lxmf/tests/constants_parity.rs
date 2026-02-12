#[test]
fn core_field_constants_match_python() {
    assert_eq!(lxmf::constants::FIELD_EMBEDDED_LXMS, 0x01);
    assert_eq!(lxmf::constants::FIELD_TELEMETRY, 0x02);
    assert_eq!(lxmf::constants::FIELD_TELEMETRY_STREAM, 0x03);
    assert_eq!(lxmf::constants::FIELD_ICON_APPEARANCE, 0x04);
    assert_eq!(lxmf::constants::FIELD_FILE_ATTACHMENTS, 0x05);
    assert_eq!(lxmf::constants::FIELD_IMAGE, 0x06);
    assert_eq!(lxmf::constants::FIELD_AUDIO, 0x07);
    assert_eq!(lxmf::constants::FIELD_THREAD, 0x08);
    assert_eq!(lxmf::constants::FIELD_COMMANDS, 0x09);
    assert_eq!(lxmf::constants::FIELD_RESULTS, 0x0A);
    assert_eq!(lxmf::constants::FIELD_GROUP, 0x0B);
    assert_eq!(lxmf::constants::FIELD_TICKET, 0x0C);
    assert_eq!(lxmf::constants::FIELD_EVENT, 0x0D);
    assert_eq!(lxmf::constants::FIELD_RNR_REFS, 0x0E);
    assert_eq!(lxmf::constants::FIELD_RENDERER, 0x0F);
}

#[test]
fn extension_field_constants_match_python() {
    assert_eq!(lxmf::constants::FIELD_CUSTOM_TYPE, 0xFB);
    assert_eq!(lxmf::constants::FIELD_CUSTOM_DATA, 0xFC);
    assert_eq!(lxmf::constants::FIELD_CUSTOM_META, 0xFD);
    assert_eq!(lxmf::constants::FIELD_NON_SPECIFIC, 0xFE);
    assert_eq!(lxmf::constants::FIELD_DEBUG, 0xFF);
}

#[test]
fn renderer_constants_match_python() {
    assert_eq!(lxmf::constants::RENDERER_PLAIN, 0x00);
    assert_eq!(lxmf::constants::RENDERER_MICRON, 0x01);
    assert_eq!(lxmf::constants::RENDERER_MARKDOWN, 0x02);
    assert_eq!(lxmf::constants::RENDERER_BBCODE, 0x03);
}

#[test]
fn audio_mode_constants_match_python() {
    assert_eq!(lxmf::constants::AM_CODEC2_450PWB, 0x01);
    assert_eq!(lxmf::constants::AM_CODEC2_450, 0x02);
    assert_eq!(lxmf::constants::AM_CODEC2_700C, 0x03);
    assert_eq!(lxmf::constants::AM_CODEC2_1200, 0x04);
    assert_eq!(lxmf::constants::AM_CODEC2_1300, 0x05);
    assert_eq!(lxmf::constants::AM_CODEC2_1400, 0x06);
    assert_eq!(lxmf::constants::AM_CODEC2_1600, 0x07);
    assert_eq!(lxmf::constants::AM_CODEC2_2400, 0x08);
    assert_eq!(lxmf::constants::AM_CODEC2_3200, 0x09);
    assert_eq!(lxmf::constants::AM_OPUS_OGG, 0x10);
    assert_eq!(lxmf::constants::AM_OPUS_LBW, 0x11);
    assert_eq!(lxmf::constants::AM_OPUS_MBW, 0x12);
    assert_eq!(lxmf::constants::AM_OPUS_PTT, 0x13);
    assert_eq!(lxmf::constants::AM_OPUS_RT_HDX, 0x14);
    assert_eq!(lxmf::constants::AM_OPUS_RT_FDX, 0x15);
    assert_eq!(lxmf::constants::AM_OPUS_STANDARD, 0x16);
    assert_eq!(lxmf::constants::AM_OPUS_HQ, 0x17);
    assert_eq!(lxmf::constants::AM_OPUS_BROADCAST, 0x18);
    assert_eq!(lxmf::constants::AM_OPUS_LOSSLESS, 0x19);
    assert_eq!(lxmf::constants::AM_CUSTOM, 0xFF);
}
