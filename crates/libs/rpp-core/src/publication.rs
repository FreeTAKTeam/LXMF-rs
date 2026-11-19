//! RDO publication references for the RPP Package, Manifest and File types
//! (RDO shared-Identity owner binding v1 revision format).

use rdo_core::revision::Revision;

use crate::cbor::Fields;
use crate::error::{Result, RppError};
use crate::identity::{Destination, Digest, PublicIdentity};
use crate::manifest::VerifiedManifest;

pub use rdo_core::revision::MAX_REVISION;
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
        let revision = Revision::verify(bytes, owner)?;
        let kind = match revision.type_iri.strip_prefix(ONTOLOGY) {
            Some("Package") => PublicationKind::Package,
            Some("Manifest") => PublicationKind::Manifest,
            Some("File") => PublicationKind::File,
            _ => return Err(RppError::InvalidPublication("unknown revision type")),
        };
        if kind != PublicationKind::File && revision.parent.is_some() {
            return Err(RppError::InvalidPublication("Package and Manifest are genesis-only"));
        }
        let mut fields = Fields::new(revision.body);
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
            object: revision.object,
            revision_id: revision.id,
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
