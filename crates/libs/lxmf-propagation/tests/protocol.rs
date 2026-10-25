use lxmf_propagation::protocol::{parse_transfer_limit_bytes, validated_binary_id_list};
use rmpv::Value;

#[test]
fn standard_get_transfer_limits_keep_python_boolean_numeric_and_utf8_behavior() {
    for (value, expected) in [
        (Value::Boolean(false), Some(0)),
        (Value::Boolean(true), Some(1000)),
        (Value::F64(1.5), Some(1500)),
        (Value::F64(-1.0), Some(0)),
        (Value::F64(f64::INFINITY), None),
        (Value::Binary(b" 2.5 ".to_vec()), Some(2500)),
    ] {
        assert_eq!(parse_transfer_limit_bytes(&value).unwrap(), expected);
    }
    assert!(parse_transfer_limit_bytes(&Value::F64(f64::NAN)).is_err());
    assert!(parse_transfer_limit_bytes(&Value::Binary(vec![255])).is_err());
    assert!(parse_transfer_limit_bytes(&Value::Nil).is_err());
}

#[test]
fn standard_ids_require_full_binary_transient_id() {
    assert_eq!(validated_binary_id_list(&[Value::Binary(vec![1; 32])]), Some(vec![vec![1; 32]]));
    assert!(validated_binary_id_list(&[Value::Binary(vec![1; 31])]).is_none());
    assert!(validated_binary_id_list(&[Value::from("11".repeat(32))]).is_none());
}
