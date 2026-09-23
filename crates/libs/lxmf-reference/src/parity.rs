use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ParityLevel {
    Complete,
    Partial,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParityRatio {
    pub numerator: u64,
    pub denominator: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParityInventory {
    pub total: u64,
    pub complete: u64,
    pub partial: u64,
    pub not_applicable: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParityCheckpoint {
    pub level: ParityLevel,
    pub complete_ratio: ParityRatio,
    pub inventory: ParityInventory,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReferenceRevision {
    pub version: String,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct SoftwareParityReferences {
    pub reticulum: ReferenceRevision,
    pub lxmf: ReferenceRevision,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct SoftwareParityOrientation {
    pub advisory: bool,
    pub references: SoftwareParityReferences,
    pub overall: ParityCheckpoint,
    pub reticulum: ParityCheckpoint,
    pub lxmf: ParityCheckpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward_behavioral: Option<BehavioralParityCheckpoint>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct BehavioralParityCheckpoint {
    pub level: ParityLevel,
    pub coverage_status: String,
    pub complete_ratio: ParityRatio,
    pub inventory: ParityInventory,
    pub evidence_verified: u64,
    pub reference: ReferenceRevision,
}

impl ParityCheckpoint {
    fn from_inventory(inventory: ParityInventory) -> Self {
        let denominator = inventory.complete + inventory.partial;
        let level = if denominator == 0 {
            ParityLevel::Unknown
        } else if inventory.partial == 0 {
            ParityLevel::Complete
        } else {
            ParityLevel::Partial
        };
        Self {
            level,
            complete_ratio: ParityRatio { numerator: inventory.complete, denominator },
            inventory,
        }
    }
}

fn inventory(
    total: usize,
    complete: usize,
    partial: usize,
    not_applicable: usize,
) -> ParityInventory {
    ParityInventory {
        total: u64::try_from(total).expect("parity inventory total fits u64"),
        complete: u64::try_from(complete).expect("parity complete count fits u64"),
        partial: u64::try_from(partial).expect("parity partial count fits u64"),
        not_applicable: u64::try_from(not_applicable)
            .expect("parity not-applicable count fits u64"),
    }
}

pub fn current_software_parity_orientation() -> SoftwareParityOrientation {
    let behavioral_inventory = inventory(
        crate::PYTHON_BEHAVIORAL_PARITY_REQUIREMENTS,
        crate::PYTHON_BEHAVIORAL_PARITY_COMPLETE,
        crate::PYTHON_BEHAVIORAL_PARITY_PARTIAL,
        crate::PYTHON_BEHAVIORAL_PARITY_NOT_APPLICABLE,
    );
    SoftwareParityOrientation {
        advisory: true,
        references: SoftwareParityReferences {
            reticulum: ReferenceRevision {
                version: crate::PYTHON_RETICULUM_REFERENCE_VERSION.to_owned(),
                revision: crate::PYTHON_RETICULUM_REFERENCE_REF.to_owned(),
            },
            lxmf: ReferenceRevision {
                version: crate::PYTHON_LXMF_REFERENCE_VERSION.to_owned(),
                revision: crate::PYTHON_LXMF_REFERENCE_REF.to_owned(),
            },
        },
        overall: ParityCheckpoint::from_inventory(inventory(
            crate::PYTHON_SOFTWARE_PARITY_TOTAL,
            crate::PYTHON_SOFTWARE_PARITY_COMPLETE,
            crate::PYTHON_SOFTWARE_PARITY_PARTIAL,
            crate::PYTHON_SOFTWARE_PARITY_NOT_APPLICABLE,
        )),
        reticulum: ParityCheckpoint::from_inventory(inventory(
            crate::PYTHON_RETICULUM_PARITY_TOTAL,
            crate::PYTHON_RETICULUM_PARITY_COMPLETE,
            crate::PYTHON_RETICULUM_PARITY_PARTIAL,
            crate::PYTHON_RETICULUM_PARITY_NOT_APPLICABLE,
        )),
        lxmf: ParityCheckpoint::from_inventory(inventory(
            crate::PYTHON_LXMF_PARITY_TOTAL,
            crate::PYTHON_LXMF_PARITY_COMPLETE,
            crate::PYTHON_LXMF_PARITY_PARTIAL,
            crate::PYTHON_LXMF_PARITY_NOT_APPLICABLE,
        )),
        forward_behavioral: Some(BehavioralParityCheckpoint {
            level: match crate::PYTHON_BEHAVIORAL_PARITY_LEVEL {
                "complete" => ParityLevel::Complete,
                "partial" => ParityLevel::Partial,
                _ => ParityLevel::Unknown,
            },
            coverage_status: crate::PYTHON_BEHAVIORAL_PARITY_COVERAGE_STATUS.to_owned(),
            complete_ratio: ParityRatio {
                numerator: behavioral_inventory.complete,
                denominator: behavioral_inventory.complete + behavioral_inventory.partial,
            },
            inventory: behavioral_inventory,
            evidence_verified: u64::try_from(crate::PYTHON_BEHAVIORAL_PARITY_VERIFIED)
                .expect("verified behavioral requirement count fits u64"),
            reference: ReferenceRevision {
                version: crate::PYTHON_BEHAVIORAL_PARITY_REFERENCE_VERSION.to_owned(),
                revision: crate::PYTHON_BEHAVIORAL_PARITY_REFERENCE_REF.to_owned(),
            },
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_orientation_reports_grouped_complete_over_applicable_ratios() {
        let orientation = current_software_parity_orientation();

        assert!(orientation.advisory);
        assert_eq!(orientation.overall.level, ParityLevel::Complete);
        assert_eq!(
            orientation.overall.complete_ratio,
            ParityRatio { numerator: 1_857, denominator: 1_857 }
        );
        assert_eq!(
            orientation.overall.inventory,
            ParityInventory { total: 1_858, complete: 1_857, partial: 0, not_applicable: 1 }
        );
        assert_eq!(orientation.reticulum.level, ParityLevel::Complete);
        assert_eq!(
            orientation.reticulum.complete_ratio,
            ParityRatio { numerator: 1_655, denominator: 1_655 }
        );
        assert_eq!(
            orientation.reticulum.inventory,
            ParityInventory { total: 1_655, complete: 1_655, partial: 0, not_applicable: 0 }
        );
        assert_eq!(orientation.lxmf.level, ParityLevel::Complete);
        assert_eq!(
            orientation.lxmf.complete_ratio,
            ParityRatio { numerator: 202, denominator: 202 }
        );
        assert_eq!(
            orientation.lxmf.inventory,
            ParityInventory { total: 202, complete: 202, partial: 0, not_applicable: 0 }
        );
        let behavioral = orientation.forward_behavioral.expect("forward advisory");
        assert_eq!(behavioral.level, ParityLevel::Partial);
        assert_eq!(behavioral.coverage_status, "incomplete");
        assert_eq!(behavioral.complete_ratio, ParityRatio { numerator: 1, denominator: 9 });
        assert_eq!(
            behavioral.inventory,
            ParityInventory { total: 10, complete: 1, partial: 8, not_applicable: 1 }
        );
        assert_eq!(behavioral.evidence_verified, 1);
        assert_eq!(behavioral.reference.version, "1.5.4-dev");
        assert_eq!(behavioral.reference.revision, "99de23c040d507e3fefca19e87b182302902725d");
    }

    #[test]
    fn empty_applicable_inventory_has_unknown_level() {
        let checkpoint = ParityCheckpoint::from_inventory(ParityInventory {
            total: 1,
            complete: 0,
            partial: 0,
            not_applicable: 1,
        });

        assert_eq!(checkpoint.level, ParityLevel::Unknown);
        assert_eq!(checkpoint.complete_ratio, ParityRatio { numerator: 0, denominator: 0 });
    }
}
