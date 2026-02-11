#[test]
fn message_sets_content_title_bytes() {
    let mut msg = lxmf::message::Message::new();
    msg.set_content_from_string("hello");
    msg.set_title_from_string("title");

    assert_eq!(msg.content_as_string().as_deref(), Some("hello"));
    assert_eq!(msg.title_as_string().as_deref(), Some("title"));
}
