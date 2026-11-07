// One generation-bound, sticky observation; no application callback runs on RX.
impl Link {
    /// Observe the first element of a MessagePack array equal to this byte.
    /// Only authenticated ordinary Link data is considered. The first observer
    /// owns the generation; reuse cannot replace it. Closure drops the producer.
    /// The first complete object is decoded with a 32-container depth budget;
    /// trailing bytes are ignored. Deeper values leave the observer armed.
    pub fn observe_msgpack_signal(
        &mut self,
        expected: u8,
    ) -> Option<tokio::sync::watch::Receiver<bool>> {
        if self.status == LinkStatus::Closed || self.data_signal.is_some() {
            return None;
        }
        let (sender, receiver) = tokio::sync::watch::channel(false);
        self.data_signal = Some((expected, sender));
        Some(receiver)
    }

    fn observe_data_signal(&self, bytes: &[u8]) {
        let Some((expected, sender)) = self.data_signal.as_ref() else {
            return;
        };
        if *sender.borrow() || sender.receiver_count() == 0 {
            return;
        }
        if msgpack_signal_matches(bytes, *expected) {
            sender.send_replace(true);
        }
    }
}

fn msgpack_signal_matches(bytes: &[u8], expected: u8) -> bool {
    // Raw rmpv preserves invalid UTF-8 for validation. A second strict
    // syntax pass rejects reserved markers inside the first object.
    // rmpv spends more than one depth unit per container. The independent
    // strict pass below enforces a 32-container safety bound.
    let Ok(value) = rmpv::decode::read_value_with_max_depth(&mut std::io::Cursor::new(bytes), 128)
    else {
        return false;
    };
    let mut decoder = rmp_serde::Deserializer::new(std::io::Cursor::new(bytes));
    decoder.set_max_depth(32);
    if <serde::de::IgnoredAny as serde::Deserialize>::deserialize(&mut decoder).is_err() {
        return false;
    }
    let rmpv::Value::Array(items) = &value else {
        return false;
    };
    if !valid_signal_value(&value) {
        return false;
    }
    match items.first() {
        Some(rmpv::Value::Integer(value)) => value.as_u64() == Some(u64::from(expected)),
        Some(rmpv::Value::F32(value)) => f64::from(*value) == f64::from(expected),
        Some(rmpv::Value::F64(value)) => *value == f64::from(expected),
        Some(rmpv::Value::Boolean(value)) => u8::from(*value) == expected,
        _ => false,
    }
}

// The reference rejects duplicate and unhashable map keys in extra array items.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum SignalKey {
    Nil,
    Positive(u64),
    Negative(i64),
    Float(u64),
    Text(String),
    Bytes(Vec<u8>),
    Ext(i8, Vec<u8>),
    Timestamp(i128),
    Tuple(Vec<SignalKey>),
}
fn signal_float_key(value: f64) -> SignalKey {
    if value.is_finite() && value.fract() == 0.0 {
        if (0.0..18446744073709551616.0).contains(&value) {
            return SignalKey::Positive(value as u64);
        }
        if (-9223372036854775808.0..0.0).contains(&value) {
            return SignalKey::Negative(value as i64);
        }
    }
    SignalKey::Float(value.to_bits())
}
fn signal_key(value: &rmpv::Value) -> Option<SignalKey> {
    match value {
        rmpv::Value::Nil => Some(SignalKey::Nil),
        rmpv::Value::Boolean(value) => Some(SignalKey::Positive(u64::from(*value))),
        rmpv::Value::Integer(value) => value
            .as_u64()
            .map(SignalKey::Positive)
            .or_else(|| value.as_i64().map(SignalKey::Negative)),
        rmpv::Value::F32(value) => Some(signal_float_key(f64::from(*value))),
        rmpv::Value::F64(value) => Some(signal_float_key(*value)),
        rmpv::Value::String(value) => value.as_str().map(|s| SignalKey::Text(s.into())),
        rmpv::Value::Binary(value) => Some(SignalKey::Bytes(value.clone())),
        rmpv::Value::Ext(-1, value) => timestamp_signal_extra(value).map(SignalKey::Timestamp),
        rmpv::Value::Ext(kind, value) => Some(SignalKey::Ext(*kind, value.clone())),
        rmpv::Value::Array(items) => {
            items.iter().map(signal_key).collect::<Option<Vec<_>>>().map(SignalKey::Tuple)
        }
        _ => None,
    }
}
fn valid_signal_value(value: &rmpv::Value) -> bool {
    match value {
        rmpv::Value::Array(items) => items.iter().all(valid_signal_value),
        rmpv::Value::Map(items) => {
            let mut keys = std::collections::BTreeSet::new();
            items.iter().all(|(key, value)| {
                let repeats_allowed = matches!(key, rmpv::Value::Array(_))
                    || matches!(key, rmpv::Value::F32(number) if number.is_nan())
                    || matches!(key, rmpv::Value::F64(number) if number.is_nan());
                signal_key(key).is_some_and(|key| repeats_allowed || keys.insert(key))
                    && valid_signal_value(value)
            })
        }
        rmpv::Value::String(value) => value.as_str().is_some(),
        rmpv::Value::Ext(-1, bytes) => timestamp_signal_extra(bytes).is_some(),
        _ => true,
    }
}

fn timestamp_signal_extra(bytes: &[u8]) -> Option<i128> {
    let (seconds, nanos) = match bytes.len() {
        4 => (i64::from(u32::from_be_bytes(bytes.try_into().expect("four bytes"))), 0),
        8 => {
            let packed = u64::from_be_bytes(bytes.try_into().expect("eight bytes"));
            ((packed & 0x3ffffffff) as i64, (packed >> 34) as u32)
        }
        12 => (
            i64::from_be_bytes(bytes[4..].try_into().expect("eight seconds bytes")),
            u32::from_be_bytes(bytes[..4].try_into().expect("four nanoseconds bytes")),
        ),
        _ => return None,
    };
    // The reference normalizes nanoseconds through timedelta, at microsecond precision.
    let micros = i128::from(seconds) * 1_000_000 + i128::from(nanos / 1000);
    (-62135596800000000..=253402300799999999).contains(&micros).then_some(micros)
}

#[cfg(test)]
mod data_signal_tests {
    use super::*;
    #[test]
    fn signal_decodes_complete_object_and_preserves_invalid_input_boundary() {
        for bytes in [
            "91ccf5",
            "91cd00f5",
            "91d100f5",
            "dc0001ccf5",
            "92ccf5c0",
            "91cb406ea00000000000",
            "91ca43750000",
            "91ccf500",
            "91ccf5c1",
            "92ccf5c401ff",
            "92ccf581910100",
        ] {
            assert!(msgpack_signal_matches(&hex::decode(bytes).expect("hex"), 245), "{bytes}");
        }
        for bytes in [
            "ccf5",
            "90",
            "91ccf4",
            "c1",
            "92ccf5",
            "92ccf5c1",
            "92ccf5a1ff",
            "92ccf58201000100",
            "92ccf5818000",
            "92ccf5d4ff00",
            "92ccf582d6ff0000000100d7ff000000000000000101",
        ] {
            assert!(!msgpack_signal_matches(&hex::decode(bytes).expect("hex"), 245), "{bytes}");
        }
        let mut nested = vec![0x92, 0xcc, 0xf5];
        nested.extend(std::iter::repeat_n(0x91, 16));
        nested.push(0xc0);
        assert!(msgpack_signal_matches(&nested, 245));
        let mut nested = vec![0x92, 0xcc, 0xf5];
        nested.extend(std::iter::repeat_n(0x91, 40));
        nested.push(0xc0);
        assert!(!msgpack_signal_matches(&nested, 245));
    }
}
