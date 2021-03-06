use crate::static_vec::{Id, StaticVec};

use ilattice3 as lat;
use std::collections::HashMap;

#[derive(Clone)]
pub struct OffsetGroup {
    offsets: OffsetMap<lat::Point>,
    offset_index: HashMap<lat::Point, OffsetId>,
}

impl OffsetGroup {
    /// `offsets` must be in order of the `OffsetId` assignments.
    pub fn new(offsets: &[lat::Point]) -> Self {
        // Build the index so users can provide `lat::Point` offsets instead of `OffsetId`s when
        // convenient.
        let offset_index: HashMap<lat::Point, OffsetId> = offsets
            .iter()
            .enumerate()
            .map(|(i, offset)| (*offset, OffsetId(i)))
            .collect();
        let offsets = OffsetMap::new(offsets.to_vec());

        OffsetGroup {
            offsets,
            offset_index,
        }
    }

    pub fn num_offsets(&self) -> usize {
        self.offsets.num_elements()
    }

    pub fn offset_id(&self, offset: &lat::Point) -> OffsetId {
        *self
            .offset_index
            .get(offset)
            .unwrap_or_else(|| panic!("Got offset {}", offset))
    }

    pub fn opposite(&self, offset: OffsetId) -> OffsetId {
        let num_offsets = self.offsets.num_elements();
        debug_assert!(num_offsets > 0);
        let max_index = num_offsets - 1;
        let opposite = max_index - offset.0;

        opposite.into()
    }

    pub fn iter(&self) -> impl Iterator<Item = (OffsetId, &lat::Point)> {
        self.offsets.iter()
    }
}

/// Represents one of the possible offsets.
#[derive(Clone, Copy, Debug)]
pub struct OffsetId(pub usize);

impl Into<usize> for OffsetId {
    fn into(self) -> usize {
        self.0
    }
}

impl From<usize> for OffsetId {
    fn from(other: usize) -> OffsetId {
        OffsetId(other)
    }
}

impl Id for OffsetId {}

// Must be ordered so opposites have mirror indices.
const FACE_3D_OFFSETS: [[i32; 3]; 6] = [
    [-1, 0, 0],
    [0, -1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [0, 1, 0],
    [1, 0, 0],
];

pub fn face_3d_offsets() -> Vec<lat::Point> {
    FACE_3D_OFFSETS
        .iter()
        .map(|o| lat::Point::from(*o))
        .collect()
}

// Must be ordered so opposites have mirror indices.
const EDGE_2D_OFFSETS: [[i32; 3]; 4] = [[-1, 0, 0], [0, -1, 0], [0, 1, 0], [1, 0, 0]];

pub fn edge_2d_offsets() -> Vec<lat::Point> {
    EDGE_2D_OFFSETS
        .iter()
        .map(|o| lat::Point::from(*o))
        .collect()
}

pub type OffsetMap<T> = StaticVec<OffsetId, T>;
