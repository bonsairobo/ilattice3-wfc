use crate::{
    pattern::{PatternId, PatternGroup, PatternSet},
    wave::Wave,
};

use ilattice3 as lat;
use ilattice3::Lattice;
use log::{debug, trace, warn};
use rand::prelude::*;

/// Generates a `Lattice<PatternId>` using the overlapping "Wave Function Collapse" algorithm.
pub struct Generator {
    rng: StdRng,
    wave: Wave,
}

impl Generator {
    pub fn new(seed: [u8; 32], output_size: lat::Point, pattern_group: &PatternGroup) -> Self {
        Generator {
            wave: Wave::new(pattern_group, output_size),
            rng: StdRng::from_seed(seed),
        }
    }

    pub fn get_wave_lattice(&self) -> &Lattice<PatternSet> {
        self.wave.get_slots()
    }

    /// Warning: undefined behavior if called before `update` returns `Success`.
    pub fn result(&self) -> Lattice<PatternId> {
        self.wave.get_slots().map(|possible_patterns: &PatternSet| {
            possible_patterns.iter().next().unwrap()
        })
    }

    pub fn update(&mut self, pattern_group: &PatternGroup) -> UpdateResult {
        let (slot, entropy) = { self.wave.choose_lowest_entropy_slot(&mut self.rng) };
        debug!(
            "{} candidate patterns remaining; chose slot {} with entropy {}",
            self.wave.get_remaining_pattern_count(), slot, entropy
        );

        if !self.observe_slot(pattern_group, &slot) {
            UpdateResult::Failure
        } else if self.wave.determined() {
            UpdateResult::Success
        } else {
            UpdateResult::Continue
        }
    }

    /// Forces `slot` to conform to a single pattern P. P is chosen by sampling from the prior
    /// distribution.
    fn observe_slot(&mut self, pattern_group: &PatternGroup, slot: &lat::Point) -> bool {
        let possible_patterns = self.wave.get_slot(slot);
        let pattern = pattern_group.sample_pattern(possible_patterns, &mut self.rng);
        debug!("Assigning {:?}", pattern);
        let removed_patterns = self.wave.collapse_slot(pattern_group, slot, pattern);
        let past_removals = removed_patterns.into_iter().map(|p| (*slot, p)).collect();

        self.propagate_constraints(&pattern_group, past_removals)
    }

    /// Returns `false` iff we find a slot with no possible patterns.
    // #[measure([ResponseTime, Throughput])]
    fn propagate_constraints(
        &mut self,
        pattern_group: &PatternGroup,
        mut past_removals: Vec<(lat::Point, PatternId)>,
    ) -> bool {
        // This algorithm is similar to flood fill, but each slot may need to be visited multiple
        // times.
        while !past_removals.is_empty() {
            // We know that this pattern is not longer possible at `visit_slot`, so no adjacent
            // patterns can use it as support.
            let (visit_slot, impossible_at_visit_slot) = past_removals.pop().unwrap();
            trace!("Visiting {} that removed {:?}", visit_slot, impossible_at_visit_slot);

            for (offset_id, offset) in pattern_group.constraints.offset_group.iter() {
                // Make sure we don't index out of bounds.
                // TODO: for PeriodicLatticeIndexer, don't worry about this
                let offset_slot = visit_slot + *offset;
                if !self.wave.get_slots().get_extent().contains_world(&offset_slot) {
                    continue;
                }

                // Remove support. We detect that a pattern is not possible in a slot if it runs out
                // of supporting adjacent patterns.
                for offset_pattern in pattern_group.constraints.iter_compatible(
                    impossible_at_visit_slot, offset_id
                ) {
                    trace!("Removing support for {:?} @ {}", offset_pattern, offset_slot);
                    let offset_support = self.wave.get_support(&offset_slot, offset_pattern);
                    let no_support = offset_support.remove(offset_id);
                    if no_support {
                        trace!("No support remaining");
                        let slot_empty = self.wave.remove_pattern(
                            pattern_group, &offset_slot, offset_pattern
                        );
                        past_removals.push((offset_slot, offset_pattern));
                        if slot_empty {
                            // Failed to fully assign the output lattice. Give up.
                            warn!("No possible patterns for {}", offset_slot);
                            return false;
                        }
                    }
                }
            }
        }

        true
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
