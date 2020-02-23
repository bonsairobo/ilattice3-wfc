use crate::{
    pattern::{PatternGroup, PatternId, PatternSampler, PatternSet},
    wave::Wave,
};

use ilattice3 as lat;
use ilattice3::Lattice;
use log::debug;
use rand::{prelude::*, rngs::SmallRng};

pub const NUM_SEED_BYTES: usize = 16;

/// Generates a `Lattice<PatternId>` using the overlapping "Wave Function Collapse" algorithm.
pub struct Generator {
    rng: SmallRng,
    wave: Wave,
}

impl Generator {
    pub fn new(
        seed: [u8; NUM_SEED_BYTES],
        output_size: lat::Point,
        pattern_sampler: &PatternSampler,
        pattern_group: &PatternGroup,
    ) -> Self {
        Generator {
            wave: Wave::new(pattern_sampler, pattern_group, output_size),
            rng: SmallRng::from_seed(seed),
        }
    }

    pub fn get_wave_lattice(&self) -> &Lattice<PatternSet> {
        self.wave.get_slots()
    }

    /// Warning: undefined behavior if called before `update` returns `Success`.
    pub fn result(&self) -> Lattice<PatternId> {
        self.wave
            .get_slots()
            .map(|possible_patterns: &PatternSet| possible_patterns.iter().next().unwrap())
    }

    pub fn num_collapsed(&self) -> usize {
        self.wave.num_collapsed()
    }

    pub fn update(
        &mut self, pattern_sampler: &PatternSampler, pattern_group: &PatternGroup
    ) -> UpdateResult {
        let (slot, entropy) = self.wave.choose_least_entropy_slot(&mut self.rng);
        debug!(
            "{} collapsed slots; chose slot {} with least entropy {}",
            self.wave.num_collapsed(),
            slot,
            entropy
        );

        if !self.wave.observe_slot(&mut self.rng, pattern_sampler, pattern_group, &slot) {
            UpdateResult::Failure
        } else if self.wave.determined() {
            UpdateResult::Success
        } else {
            UpdateResult::Continue
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum UpdateResult {
    /// The output lattice is fully assigned.
    Success,
    /// Further calls to `update` are required.
    Continue,
    /// The currently assigned patterns cannot satisfy the constraints.
    /// TODO: backtracking can help alleviate this
    Failure,
}
