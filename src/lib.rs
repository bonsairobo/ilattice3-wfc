//! Implementation of Max Gumin's "Wave Function Collapse" algorithm for voxel maps.

// TODO: mirror and rotational symmetries

// TODO: backtracking
// The plan is to keep a log of collapse choices and for each one, a log of removals. Then the
// remove_pattern operation needs to be made reversible. Then to reverse a collapse, we reverse all
// of the removals that happened since, then choose a new collapse.

#![feature(map_first_last)]

mod generate;
mod image;
mod offset;
mod pattern;
mod static_vec;
mod wave;

pub use crate::image::{
    color_final_patterns_rgba, color_final_patterns_vox, color_superposition, make_palette_lattice,
    GifMaker,
};
pub use generate::{Generator, UpdateResult, NUM_SEED_BYTES};
pub use offset::{edge_2d_offsets, face_3d_offsets, OffsetGroup};
pub use pattern::{
    find_unique_tiles, process_patterns_in_lattice, PatternConstraints, PatternId, PatternMap,
    PatternSampler, PatternSet, PatternShape,
};

use ::image::ImageError;
use ilattice3::VecLatticeMap;
use std::error;
use std::fmt;
use std::io;

pub trait FrameConsumer {
    fn use_frame(&mut self, frame: &VecLatticeMap<PatternSet>);
}

pub struct NilFrameConsumer;

impl FrameConsumer for NilFrameConsumer {
    fn use_frame(&mut self, _frame: &VecLatticeMap<PatternSet>) {}
}

#[derive(Debug)]
pub enum CliError {
    ImageError(ImageError),
    IoError(io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CliError::ImageError(e) => write!(f, "{}", e),
            CliError::IoError(e) => write!(f, "{}", e),
        }
    }
}

impl error::Error for CliError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            CliError::ImageError(e) => e.source(),
            CliError::IoError(e) => e.source(),
        }
    }
}

impl From<io::Error> for CliError {
    fn from(e: io::Error) -> Self {
        CliError::IoError(e)
    }
}

impl From<ImageError> for CliError {
    fn from(e: ImageError) -> Self {
        CliError::ImageError(e)
    }
}
