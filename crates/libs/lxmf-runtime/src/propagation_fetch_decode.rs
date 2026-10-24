//! Existing REM response compatibility, hosted by the compiled protocol client.
use crate::delivery::transport_error;
use lxmf_sdk::SdkError;

fn value_to_bytes(value: &rmpv::Value) -> Option<Vec<u8>> {
    match value {
        rmpv::Value::Binary(bytes) => Some(bytes.clone()),
        rmpv::Value::String(text) => {
            let value = text.as_str()?;
            if let Ok(decoded) = hex::decode(value) {
                return Some(decoded);
            }
            Some(value.as_bytes().to_vec())
        }
        _ => None,
    }
}

fn rmpv_propagation_envelope_payloads(value: &rmpv::Value) -> Option<Vec<Vec<u8>>> {
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    if entries.len() < 2 {
        return None;
    }
    let timestamp_like = matches!(
        entries.first(),
        Some(rmpv::Value::F32(_)) | Some(rmpv::Value::F64(_)) | Some(rmpv::Value::Integer(_))
    );
    if !timestamp_like {
        return None;
    }
    let rmpv::Value::Array(payloads) = &entries[1] else {
        return None;
    };
    let decoded = payloads.iter().map(value_to_bytes).collect::<Option<Vec<_>>>()?;
    (!decoded.is_empty()).then_some(decoded)
}

fn propagation_payloads_from_bytes(bytes: &[u8]) -> Vec<Vec<u8>> {
    if let Ok(value) = rmp_serde::from_slice::<rmpv::Value>(bytes) {
        if let Some(payloads) = rmpv_propagation_envelope_payloads(&value) {
            return payloads;
        }
    }
    vec![bytes.to_vec()]
}

fn propagation_payloads_from_fetch_entry(value: &rmpv::Value) -> Result<Vec<Vec<u8>>, SdkError> {
    if let Some(payloads) = rmpv_propagation_envelope_payloads(value) {
        return Ok(payloads);
    }
    match value {
        rmpv::Value::Binary(bytes) => Ok(propagation_payloads_from_bytes(bytes)),
        rmpv::Value::String(text) => {
            let value =
                text.as_str().ok_or(transport_error("invalid propagation payload envelope"))?;
            let bytes = hex::decode(value).unwrap_or_else(|_| value.as_bytes().to_vec());
            Ok(propagation_payloads_from_bytes(bytes.as_slice()))
        }
        rmpv::Value::Array(entries) => {
            if entries.len() >= 2 {
                if let Some(payloads) = rmpv_propagation_envelope_payloads(&entries[1]) {
                    return Ok(payloads);
                }
                if let Some(bytes) = entries.get(1).and_then(value_to_bytes) {
                    return Ok(propagation_payloads_from_bytes(bytes.as_slice()));
                }
            }
            Err(transport_error("invalid propagation payload envelope"))
        }
        _ => Err(transport_error("invalid propagation payload envelope")),
    }
}

pub(super) fn payloads(value: &rmpv::Value) -> Result<Vec<Vec<u8>>, SdkError> {
    if let Some(payloads) = rmpv_propagation_envelope_payloads(value) {
        return Ok(payloads);
    }
    let rmpv::Value::Array(values) = value else {
        return Err(transport_error("invalid propagation payload envelope"));
    };
    let mut payloads = Vec::new();
    for value in values {
        payloads.extend(propagation_payloads_from_fetch_entry(value)?);
    }
    Ok(payloads)
}
