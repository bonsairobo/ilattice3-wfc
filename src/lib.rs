//! Implementation of Max Gumin's "Wave Function Collapse" algorithm for voxel maps.

#![feature(map_first_last)]

mod generate;
mod image;
mod offset;
mod pattern;
mod static_vec;
mod wave;

pub use crate::image::{
    color_final_patterns, color_superposition, image_from_lattice, lattice_from_image,
    make_palette_lattice,
};
pub use generate::{Generator, UpdateResult};
pub use offset::{edge_2d_offsets, face_3d_offsets, OffsetGroup};
pub use pattern::{
    find_pattern_colors, process_patterns_in_lattice, PatternColors, PatternId, PatternGroup,
    PatternShape, SymmetricPatternConstraints,
};
