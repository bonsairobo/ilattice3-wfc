use ilattice3 as lat;
use std::collections::HashMap;

#[derive(Clone)]
pub struct OffsetGroup {
    offsets: Vec<lat::Point>,
    offset_index: HashMap<lat::Point, usize>,
}

impl OffsetGroup {
    /// `offsets` must be in order of the `OffsetId` assignments.
    pub fn new(offsets: &[lat::Point]) -> Self {
        // Build the index so users can provide `lat::Point` offsets instead of `OffsetId`s when
        // convenient.
        let offset_index: HashMap<lat::Point, usize> = offsets
            .iter()
            .enumerate()
            .map(|(i, offset)| (*offset, i))
            .collect();
        let offsets = offsets.to_vec();

        OffsetGroup {
            offsets,
            offset_index,
        }
    }

    pub fn num_offsets(&self) -> usize {
        self.offset_index.len()
    }

    pub fn offset_id(&self, offset: &lat::Point) -> OffsetId {
        OffsetId(
            *self
                .offset_index
                .get(offset)
                .unwrap_or_else(|| panic!("Got offset {}", offset)),
        )
    }

    pub fn iter(&self) -> impl Iterator<Item = (OffsetId, &lat::Point)> {
        self.offsets
            .iter()
            .enumerate()
            .map(|(i, o)| (OffsetId(i), o))
    }
}

/// Represents one of the possible offsets.
#[derive(Clone, Copy, Debug)]
pub struct OffsetId(pub usize);

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

const EDGE_2D_OFFSETS: [[i32; 3]; 4] = [[-1, 0, 0], [0, -1, 0], [0, 1, 0], [1, 0, 0]];

pub fn edge_2d_offsets() -> Vec<lat::Point> {
    EDGE_2D_OFFSETS
        .iter()
        .map(|o| lat::Point::from(*o))
        .collect()
}
