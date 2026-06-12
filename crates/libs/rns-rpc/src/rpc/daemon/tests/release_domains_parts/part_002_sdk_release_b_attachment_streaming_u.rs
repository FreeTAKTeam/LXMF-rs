#[test]
fn sdk_release_b_attachment_streaming_upload_resume_and_integrity() {
    let daemon = RpcDaemon::test_instance();
    let topic = daemon
        .handle_rpc(rpc_request(116, "sdk_topic_create_v2", json!({ "topic_path": "ops/chunks" })))
        .expect("topic create");
    let topic_id = topic.result.expect("topic result")["topic"]["topic_id"]
        .as_str()
        .expect("topic id")
        .to_string();
    let payload = b"chunked-attachment-payload".to_vec();
    let checksum = encode_hex(Sha256::digest(payload.as_slice()));

    let upload_start = daemon
        .handle_rpc(rpc_request(
            117,
            "sdk_attachment_upload_start_v2",
            json!({
                "name": "chunked.bin",
                "content_type": "application/octet-stream",
                "total_size": payload.len(),
                "checksum_sha256": checksum,
                "topic_ids": [topic_id],
            }),
        ))
        .expect("upload start");
    assert!(upload_start.error.is_none());
    let upload = upload_start.result.expect("result")["upload"].clone();
    let upload_id = upload["upload_id"].as_str().expect("upload_id").to_string();
    let attachment_id = upload["attachment_id"].as_str().expect("attachment_id").to_string();

    let first_chunk = &payload[..8];
    let second_chunk = &payload[8..];
    let chunk_1 = daemon
        .handle_rpc(rpc_request(
            118,
            "sdk_attachment_upload_chunk_v2",
            json!({
                "upload_id": upload_id,
                "offset": 0,
                "bytes_base64": BASE64_STANDARD.encode(first_chunk),
            }),
        ))
        .expect("chunk 1");
    assert!(chunk_1.error.is_none());
    assert_eq!(chunk_1.result.expect("result")["upload_chunk"]["next_offset"], json!(8));

    let chunk_2 = daemon
        .handle_rpc(rpc_request(
            119,
            "sdk_attachment_upload_chunk_v2",
            json!({
                "upload_id": upload["upload_id"].as_str().expect("upload_id"),
                "offset": 8,
                "bytes_base64": BASE64_STANDARD.encode(second_chunk),
            }),
        ))
        .expect("chunk 2");
    assert!(chunk_2.error.is_none());
    assert_eq!(chunk_2.result.expect("result")["upload_chunk"]["complete"], json!(true));

    let commit = daemon
        .handle_rpc(rpc_request(
            120,
            "sdk_attachment_upload_commit_v2",
            json!({ "upload_id": upload["upload_id"].as_str().expect("upload_id") }),
        ))
        .expect("upload commit");
    assert!(commit.error.is_none());
    assert_eq!(
        commit.result.expect("result")["attachment"]["attachment_id"],
        json!(attachment_id.clone())
    );

    let download_chunk = daemon
        .handle_rpc(rpc_request(
            121,
            "sdk_attachment_download_chunk_v2",
            json!({
                "attachment_id": attachment_id.clone(),
                "offset": 0,
                "max_bytes": 5,
            }),
        ))
        .expect("download chunk");
    assert!(download_chunk.error.is_none());
    let chunk_result = download_chunk.result.expect("result")["download_chunk"].clone();
    assert_eq!(chunk_result["attachment_id"], json!(attachment_id));
    assert_eq!(chunk_result["offset"], json!(0));
    assert_eq!(chunk_result["next_offset"], json!(5));
    assert_eq!(chunk_result["done"], json!(false));
    assert_eq!(
        BASE64_STANDARD
            .decode(chunk_result["bytes_base64"].as_str().expect("chunk base64").as_bytes())
            .expect("decode chunk"),
        payload[..5]
    );
}

#[test]
fn sdk_release_b_attachment_streaming_commit_rejects_checksum_mismatch() {
    let daemon = RpcDaemon::test_instance();
    let upload_start = daemon
            .handle_rpc(rpc_request(
                122,
                "sdk_attachment_upload_start_v2",
                json!({
                    "name": "bad.bin",
                    "content_type": "application/octet-stream",
                    "total_size": 4,
                    "checksum_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                }),
            ))
            .expect("upload start");
    assert!(upload_start.error.is_none());
    let upload_id = upload_start.result.expect("result")["upload"]["upload_id"]
        .as_str()
        .expect("upload_id")
        .to_string();

    let chunk = daemon
        .handle_rpc(rpc_request(
            123,
            "sdk_attachment_upload_chunk_v2",
            json!({
                "upload_id": upload_id.clone(),
                "offset": 0,
                "bytes_base64": BASE64_STANDARD.encode([1_u8, 2, 3, 4]),
            }),
        ))
        .expect("upload chunk");
    assert!(chunk.error.is_none());

    let commit = daemon
        .handle_rpc(rpc_request(
            124,
            "sdk_attachment_upload_commit_v2",
            json!({ "upload_id": upload_id }),
        ))
        .expect("upload commit");
    let error = commit.error.expect("checksum mismatch error");
    assert_eq!(error.code, "SDK_VALIDATION_CHECKSUM_MISMATCH");
}
