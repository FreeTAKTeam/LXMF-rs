#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PaperDecodeResult {
    pub accepted: bool,
    pub transient_id: String,
    pub duplicate: bool,
    pub destination: String,
    pub destination_hint: String,
    pub bytes_len: u64,
    #[serde(default)]
    pub revision: Option<u64>,
}

impl PaperDecodeResult {
    pub fn ack(&self) -> crate::types::Ack {
        crate::types::Ack {
            accepted: self.accepted,
            revision: self.revision,
        }
    }
}

#[cfg(test)]
mod paper_decode_result_tests {
    use super::PaperDecodeResult;

    #[test]
    fn ack_preserves_acceptance_and_revision() {
        let result = PaperDecodeResult {
            accepted: true,
            transient_id: "paper-msg-1".to_owned(),
            duplicate: false,
            destination: "dest-hash".to_owned(),
            destination_hint: "dest-hash".to_owned(),
            bytes_len: 72,
            revision: Some(7),
        };

        let ack = result.ack();

        assert!(ack.accepted);
        assert_eq!(ack.revision, Some(7));
    }
}
