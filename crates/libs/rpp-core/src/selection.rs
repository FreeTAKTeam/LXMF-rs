//! Chunk selections: sorted, nonoverlapping indexes and inclusive ranges.

use crate::cbor::{self, Value};
use crate::error::{Result, RppError};

pub const MAX_SELECTION_ENTRIES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionEntry {
    Index(u32),
    Range(u32, u32),
}

impl SelectionEntry {
    fn bounds(self) -> (u32, u32) {
        match self {
            Self::Index(index) => (index, index),
            Self::Range(first, last) => (first, last),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection(pub Vec<SelectionEntry>);

impl Selection {
    /// Structural validation happens before any expansion.
    pub fn decode(values: Vec<Value>) -> Result<Self> {
        if values.len() > MAX_SELECTION_ENTRIES {
            return Err(RppError::LimitExceeded);
        }
        let mut entries = Vec::with_capacity(values.len());
        let mut next_free: u64 = 0;
        for value in values {
            let entry = match value {
                Value::Uint(index) => SelectionEntry::Index(index_u32(index)?),
                Value::Array(pair) => match pair.as_slice() {
                    [first, last] => SelectionEntry::Range(
                        index_u32(first.as_uint()?)?,
                        index_u32(last.as_uint()?)?,
                    ),
                    _ => return Err(RppError::InvalidSelection),
                },
                _ => return Err(RppError::InvalidSelection),
            };
            let (first, last) = entry.bounds();
            if first > last || u64::from(first) < next_free {
                return Err(RppError::InvalidSelection);
            }
            next_free = u64::from(last) + 1;
            entries.push(entry);
        }
        Ok(Self(entries))
    }

    pub fn encode(&self) -> Value {
        Value::Array(
            self.0
                .iter()
                .map(|entry| match *entry {
                    SelectionEntry::Index(index) => Value::Uint(u64::from(index)),
                    SelectionEntry::Range(first, last) => {
                        cbor::uint_array([u64::from(first), u64::from(last)])
                    }
                })
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn check_bounds(&self, chunk_count: u32) -> Result<()> {
        match self.0.last() {
            Some(entry) if entry.bounds().1 >= chunk_count => Err(RppError::InvalidSelection),
            _ => Ok(()),
        }
    }

    /// Number of selected indexes, without expanding ranges.
    pub fn count(&self) -> u64 {
        self.0.iter().map(|e| u64::from(e.bounds().1 - e.bounds().0) + 1).sum()
    }

    pub fn indexes(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().flat_map(|entry| {
            let (first, last) = entry.bounds();
            first..=last
        })
    }

    /// Compresses ascending indexes into at most `max_entries` entries; the
    /// rest are left for a later request (a bounded subset is allowed).
    pub fn from_sorted(indexes: impl IntoIterator<Item = u32>, max_entries: usize) -> Self {
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        for index in indexes {
            let full = ranges.len() == max_entries;
            match ranges.last_mut() {
                Some((_, last)) if last.checked_add(1) == Some(index) => *last = index,
                _ if full => break,
                _ => ranges.push((index, index)),
            }
        }
        Self(
            ranges
                .into_iter()
                .map(|(first, last)| {
                    if first == last {
                        SelectionEntry::Index(first)
                    } else {
                        SelectionEntry::Range(first, last)
                    }
                })
                .collect(),
        )
    }
}

fn index_u32(index: u64) -> Result<u32> {
    u32::try_from(index).map_err(|_| RppError::InvalidSelection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections_compress_validate_and_expand() {
        let selection = Selection::from_sorted([1, 2, 3, 7, 9, 10], 256);
        assert_eq!(
            selection.0,
            vec![
                SelectionEntry::Range(1, 3),
                SelectionEntry::Index(7),
                SelectionEntry::Range(9, 10)
            ]
        );
        assert_eq!(selection.count(), 6);
        assert_eq!(selection.indexes().collect::<Vec<_>>(), vec![1, 2, 3, 7, 9, 10]);
        assert_eq!(
            Selection::decode(selection.encode().into_array().expect("test step")),
            Ok(selection.clone())
        );
        assert_eq!(selection.check_bounds(10), Err(RppError::InvalidSelection));
        assert_eq!(Selection::from_sorted([1, 3, 5], 2).0.len(), 2);
        let overlapping = vec![cbor::uint_array([0, 5]), Value::Uint(5)];
        assert_eq!(Selection::decode(overlapping), Err(RppError::InvalidSelection));
    }
}
