include!("wire_fields_parts/module_prelude.rs");

include!("wire_fields_parts/enrich_app_extension_fields.rs");

#[cfg(test)]
mod attachment_tests {
    use super::*;

    #[test]
    fn file_field_uses_binary_payload_compatible_with_lxmf() {
        let fields = serde_json::json!({"5":[["hello.txt",[104,101,108,108,111]]]});
        let encoded = json_to_rmpv(&fields).expect("wire fields");
        let Value::Map(root) = &encoded else { panic!("field map") };
        let (_, Value::Array(files)) = root.first().expect("file field") else { panic!("files") };
        let Value::Array(pair) = &files[0] else { panic!("file pair") };
        assert_eq!(pair[1], Value::Binary(b"hello".to_vec()));
        assert_eq!(rmpv_to_json(&encoded).expect("stored fields"), fields);
    }

    #[test]
    fn image_field_uses_binary_payload_compatible_with_lxmf() {
        let fields = serde_json::json!({"6":["jpg",[255,216,255]]});
        let encoded = json_to_rmpv(&fields).expect("wire fields");
        let Value::Map(root) = &encoded else { panic!("field map") };
        let (_, Value::Array(image)) = root.first().expect("image field") else { panic!("image") };
        assert_eq!(image[1], Value::Binary(vec![255, 216, 255]));
        assert_eq!(rmpv_to_json(&encoded).expect("stored fields"), fields);
    }
}
