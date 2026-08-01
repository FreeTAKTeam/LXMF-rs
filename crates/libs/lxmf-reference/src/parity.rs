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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_orientation_reports_grouped_complete_over_applicable_ratios() {
        let orientation = current_software_parity_orientation();

        assert!(orientation.advisory);
        assert_eq!(orientation.overall.level, ParityLevel::Partial);
        assert_eq!(
            orientation.overall.complete_ratio,
            ParityRatio { numerator: 1_695, denominator: 1_810 }
        );
        assert_eq!(
            orientation.overall.inventory,
            ParityInventory { total: 1_811, complete: 1_695, partial: 115, not_applicable: 1 }
        );
        assert_eq!(orientation.reticulum.level, ParityLevel::Partial);
        assert_eq!(
            orientation.reticulum.complete_ratio,
            ParityRatio { numerator: 1_493, denominator: 1_608 }
        );
        assert_eq!(
            orientation.reticulum.inventory,
            ParityInventory { total: 1_608, complete: 1_493, partial: 115, not_applicable: 0 }
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
