use crate::{EmbeddedError, EmbeddedResult};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EmbeddedIdentity {
    pub id: [u8; 32],
    pub verify_key: [u8; 32],
}

impl EmbeddedIdentity {
    pub fn new(id: [u8; 32], verify_key: [u8; 32]) -> EmbeddedResult<Self> {
        if id == [0; 32] || verify_key == [0; 32] {
            return Err(EmbeddedError::InvalidInput);
        }
        Ok(Self { id, verify_key })
    }
}

pub trait IdentityProvider {
    fn active_identity(&self) -> EmbeddedResult<EmbeddedIdentity>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Signature(pub [u8; 64]);

pub trait PayloadSigner {
    fn sign(&self, payload: &[u8]) -> EmbeddedResult<Signature>;
}

pub trait PayloadVerifier {
    fn verify(&self, payload: &[u8], signature: &Signature) -> EmbeddedResult<bool>;
}

pub fn sign_payload(signer: &dyn PayloadSigner, payload: &[u8]) -> EmbeddedResult<Signature> {
    if payload.is_empty() {
        return Err(EmbeddedError::InvalidInput);
    }
    signer.sign(payload)
}

pub fn verify_payload(
    verifier: &dyn PayloadVerifier,
    payload: &[u8],
    signature: &Signature,
) -> EmbeddedResult<bool> {
    if payload.is_empty() {
        return Err(EmbeddedError::InvalidInput);
    }
    verifier.verify(payload, signature)
}

#[cfg(test)]
mod tests {
    use super::{sign_payload, verify_payload, PayloadSigner, PayloadVerifier, Signature};
    use crate::{hash::digest32, EmbeddedError, EmbeddedResult};

    struct FakeCrypto;

    impl PayloadSigner for FakeCrypto {
        fn sign(&self, payload: &[u8]) -> EmbeddedResult<Signature> {
            let digest = digest32(payload);
            let mut sig = [0_u8; 64];
            sig[..32].copy_from_slice(&digest);
            sig[32..].copy_from_slice(&digest);
            Ok(Signature(sig))
        }
    }

    impl PayloadVerifier for FakeCrypto {
        fn verify(&self, payload: &[u8], signature: &Signature) -> EmbeddedResult<bool> {
            let expected = self.sign(payload)?;
            Ok(signature == &expected)
        }
    }

    #[test]
    fn sign_then_verify() {
        let crypto = FakeCrypto;
        let payload = b"embedded-node";
        let signature = sign_payload(&crypto, payload).expect("sign");
        assert!(verify_payload(&crypto, payload, &signature).expect("verify"));
        assert!(!verify_payload(&crypto, b"different", &signature).expect("verify mismatch"));
    }

    #[test]
    fn empty_payload_is_rejected() {
        let crypto = FakeCrypto;
        let err = sign_payload(&crypto, b"").expect_err("empty payload rejected");
        assert_eq!(err, EmbeddedError::InvalidInput);
    }
}
