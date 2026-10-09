//! Borrowed preflight: reject structural expansion before allocating JSON trees.
use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
struct Seed<'a> {
    remaining: &'a mut usize,
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, decoder: D) -> Result<(), D::Error> {
        if self.depth > 64 || *self.remaining == 0 {
            return Err(D::Error::custom("ZeroMQ request exceeds structural decode budget"));
        }
        *self.remaining -= 1;
        decoder.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("bounded RPC value")
    }
    fn visit_bool<E: Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_bytes<E: Error>(self, _: &[u8]) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_none<E: Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut values: A) -> Result<(), A::Error> {
        while values
            .next_element_seed(Seed { remaining: self.remaining, depth: self.depth + 1 })?
            .is_some()
        {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut values: A) -> Result<(), A::Error> {
        while values
            .next_key_seed(Seed { remaining: self.remaining, depth: self.depth + 1 })?
            .is_some()
        {
            values.next_value_seed(Seed { remaining: self.remaining, depth: self.depth + 1 })?;
        }
        Ok(())
    }
}
pub(super) fn check(bytes: &[u8]) -> std::io::Result<usize> {
    let mut remaining = 65536;
    let mut decoder = rmp_serde::Deserializer::from_read_ref(bytes);
    Seed { remaining: &mut remaining, depth: 0 }
        .deserialize(&mut decoder)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(65536 - remaining)
}
#[cfg(test)]
mod tests {
    #[test]
    fn compact_array_is_rejected_without_expansion() {
        let bytes = rmp_serde::to_vec(&vec![0; 400_000]).unwrap();
        assert!(super::check(&bytes).is_err());
        assert!(super::check(
            &rmp_serde::to_vec(&serde_json::json!({"content":"Test1234","fields":[1,2,3]}))
                .unwrap()
        )
        .is_ok());
    }
}
