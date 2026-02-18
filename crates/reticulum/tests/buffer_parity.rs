use reticulum::buffer::{InputBuffer, OutputBuffer, StaticBuffer};

#[test]
fn static_buffer_write_and_rotate() {
    let mut buf: StaticBuffer<8> = StaticBuffer::new();
    buf.write(&[1, 2, 3, 4]).unwrap();
    assert_eq!(buf.as_slice(), &[1, 2, 3, 4]);

    buf.rotate_left(2).unwrap();
    assert_eq!(buf.as_slice(), &[3, 4]);

    buf.chain_safe_write(&[5, 6, 7]);
    assert_eq!(buf.as_slice(), &[3, 4, 5, 6, 7]);
}

#[test]
fn output_buffer_writes_bytes() {
    let mut out = [0u8; 4];
    let mut buf = OutputBuffer::new(&mut out);
    buf.write_byte(0xAA).unwrap();
    buf.write(&[0xBB, 0xCC]).unwrap();
    assert_eq!(buf.as_slice(), &[0xAA, 0xBB, 0xCC]);
}

#[test]
fn input_buffer_reads() {
    let data = [0x10u8, 0x20, 0x30];
    let mut buf = InputBuffer::new(&data);
    assert_eq!(buf.read_byte().unwrap(), 0x10);
    let slice = buf.read_slice(2).unwrap();
    assert_eq!(slice, &[0x20, 0x30]);
}
