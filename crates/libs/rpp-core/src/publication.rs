//! RDO publication references for the RPP Package, Manifest and File types
//! (RDO shared-Identity owner binding v1 revision format).

use crate::cbor::{self, Fields, Value};
use crate::error::{Result, RppError};
use crate::identity::{sha256, Destination, Digest, PublicIdentity};
use crate::manifest::VerifiedManifest;

pub const MAX_REVISION: usize = 48 * 1024;
const ONTOLOGY: &str = "https://freetakteam.github.io/rpp/ontology#";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationKind {
    Package,
    Manifest,
    File,
}

/// A verified, owner-signed publication reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publication {
    pub kind: PublicationKind,
    pub object: Destination,
    /// RDO revision ID: SHA-256 of the whole signed revision.
    pub revision_id: Digest,
    pub package_destination: Destination,
    pub manifest_destination: Destination,
    pub package_id: Digest,
    pub manifest_hash: Digest,
    /// File digest and path, for File revisions only.
    pub file: Option<(Digest, String)>,
}

impl Publication {
    /// Verifies the revision signature against the resolved owner Identity.
    pub fn verify(bytes: &[u8], owner: &PublicIdentity) -> Result<Self> {
        let items = cbor::decode(bytes, MAX_REVISION, true)?.into_array()?;
        let [version, object, parent, kind, body, signature]: [Value; 6] =
            items.try_into().map_err(|_| RppError::Malformed("revision must have six items"))?;
        if version.as_uint()? != 1 {
            return Err(RppError::UnsupportedVersion(version.as_uint()?));
        }
        let signed = cbor::encode(&Value::Array(vec![
            Value::Text("rdo.revision.v1".into()),
            Value::Uint(1),
            object.clone(),
            parent.clone(),
            kind.clone(),
            body.clone(),
        ]));
        owner.verify(&signed, &signature.into_fixed()?)?;

        let kind = match kind.into_text()?.strip_prefix(ONTOLOGY) {
            Some("Package") => PublicationKind::Package,
            Some("Manifest") => PublicationKind::Manifest,
            Some("File") => PublicationKind::File,
            _ => return Err(RppError::InvalidPublication("unknown revision type")),
        };
        let parent_is_null = parent == Value::Null;
        if !parent_is_null && parent.into_fixed::<32>().is_err() {
            return Err(RppError::Malformed("revision parent"));
        }
        if kind != PublicationKind::File && !parent_is_null {
            return Err(RppError::InvalidPublication("Package and Manifest are genesis-only"));
        }
        let mut fields = Fields::new(body.into_map()?);
        let package_destination = fields.fixed(0)?;
        let manifest_destination = fields.fixed(1)?;
        let package_id = fields.fixed(2)?;
        let manifest_hash = fields.fixed(3)?;
        let file = match kind {
            PublicationKind::File => Some((fields.fixed(4)?, fields.required(5)?.into_text()?)),
            _ => None,
        };
        fields.finish()?;
        Ok(Self {
            kind,
            object: object.into_fixed()?,
            revision_id: sha256(bytes),
            package_destination,
            manifest_destination,
            package_id,
            manifest_hash,
            file,
        })
    }

    /// Compares this reference with a verified manifest before first acceptance.
    pub fn check_manifest(&self, verified: &VerifiedManifest) -> Result<()> {
        let manifest = &verified.manifest;
        let snapshot_matches = self.package_destination == manifest.package_destination
            && self.manifest_destination == manifest.manifest_destination
            && self.package_id == manifest.package_id
            && self.manifest_hash == verified.hash;
        let object_matches = match (&self.kind, &self.file) {
            (PublicationKind::Package, _) => self.object == manifest.package_destination,
            (PublicationKind::Manifest, _) => self.object == manifest.manifest_destination,
            (PublicationKind::File, Some((digest, path))) => manifest.files.iter().any(|file| {
                file.destination == self.object && file.digest == *digest && file.path == *path
            }),
            (PublicationKind::File, None) => false,
        };
        if snapshot_matches && object_matches {
            Ok(())
        } else {
            Err(RppError::InvalidPublication("reference does not match the signed manifest"))
        }
    }
}
