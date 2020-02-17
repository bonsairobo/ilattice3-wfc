use crate::{
    offset::OffsetId,
    pattern::{PatternId, PatternGroup},
    wave::Wave,
};

use hibitset::{BitSet, BitSetLike};
use ilattice3 as lat;
use ilattice3::Lattice;
use log::{debug, warn};
use rand::prelude::*;
use std::collections::BTreeSet;

/// Generates a `Lattice<PatternId>` using the overlapping "Wave Function Collapse" algorithm.
pub struct Generator {
    rng: StdRng,
    wave: Wave,
}

impl Generator {
    pub fn new(seed: [u8; 32], output_size: lat::Point, patterns: &PatternGroup) -> Self {
        Generator {
            wave: Wave::new(patterns, output_size),
            rng: StdRng::from_seed(seed),
        }
    }

    pub fn get_wave_lattice(&self) -> &Lattice<BitSet> {
        self.wave.get_slots()
    }

    /// Warning: undefined behavior if called before `update` returns `Success`.
    pub fn result(&self) -> Lattice<PatternId> {
        self.wave.get_slots().map(|possible_patterns: &BitSet| {
            let only_pattern: u32 = possible_patterns.iter().next().unwrap();

            PatternId(only_pattern)
        })
    }

    pub fn update(&mut self, patterns: &PatternGroup) -> UpdateResult {
        let (slot, entropy) = { self.wave.choose_lowest_entropy_slot(&mut self.rng) };
        debug!(
            "{} candidate patterns remaining; chose slot {} with entropy {}",
            self.wave.get_remaining_pattern_count(), slot, entropy
        );

        if !self.observe_slot(patterns, &slot) {
            UpdateResult::Failure
        } else if self.wave.determined() {
            UpdateResult::Success
        } else {
            UpdateResult::Continue
        }
    }

    /// Forces `slot` to conform to a single pattern P. P is chosen by sampling from the prior
    /// distribution.
    fn observe_slot(&mut self, patterns: &PatternGroup, slot: &lat::Point) -> bool {
        let possible_patterns = self.wave.get_slot(slot);
        let pattern = patterns.sample_pattern(possible_patterns, &mut self.rng);
        self.wave.collapse_slot(patterns, slot, pattern);

        self.propagate_constraints(patterns, *slot)
    }

    /// Returns `false` iff we find a slot with no possible patterns.
    // #[measure([ResponseTime, Throughput])]
    fn propagate_constraints(
        &mut self,
        pattern_set: &PatternGroup,
        changed_slot: lat::Point,
    ) -> bool {
        let mut slots_to_visit = BTreeSet::new();
        // Point is not Ord, so just do a no-op conversion for this container.
        slots_to_visit.insert(<[i32; 3]>::from(changed_slot));

        // This algorithm is similar to flood fill, but it's possible that we need to visit a slot
        // multiple times if multiple of its offset slots change.
        let mut impossible_patterns = Vec::new();
        while !slots_to_visit.is_empty() {
            let visit_slot: lat::Point = slots_to_visit.pop_last().unwrap().into();

            for (offset_id, offset) in pattern_set.constraints.offset_group.iter() {
                // Make sure we don't index out of bounds.
                let offset_slot = visit_slot + *offset;
                if !self.wave.get_slots().get_extent().contains_world(&offset_slot) {
                    continue;
                }

                let any_possible = self.remove_impossible_patterns(
                    pattern_set,
                    &visit_slot,
                    &offset_slot,
                    offset_id,
                    &mut impossible_patterns,
                );
                if !any_possible {
                    // Failed to fully assign the output lattice. Give up.
                    warn!("No possible patterns for {}", offset_slot);
                    return false;
                }

                // Possible patterns changed, so we need to propagate this change.
                if !impossible_patterns.is_empty() {
                    slots_to_visit.insert(<[i32; 3]>::from(offset_slot));
                }
            }
        }

        true
    }

    fn remove_impossible_patterns(
        &mut self,
        pattern_set: &PatternGroup,
        visit_slot: &lat::Point,
        offset_slot: &lat::Point,
        offset_id: OffsetId,
        impossible_patterns: &mut Vec<u32>,
    ) -> bool {
        impossible_patterns.clear();

        // See if the set of possible patterns at `offset_slot` has changed.
        let possible_offset_patterns = self.wave.get_slot(&offset_slot);
        for offset_pattern in possible_offset_patterns.iter() {
            // `pattern` is not possible if it's not compatible with any of the possibilities at
            // `visit_slot`.
            let possible_visit_slot_patterns = self.wave.get_slot(&visit_slot);
            let still_possible = possible_visit_slot_patterns.iter().any(|visit_pattern| {
                pattern_set.constraints.compatible(
                    offset_id,
                    PatternId(visit_pattern),
                    PatternId(offset_pattern),
                )
            });
            if !still_possible {
                impossible_patterns.push(offset_pattern);
            }
        }

        for remove_pattern in impossible_patterns.iter() {
            self.wave
                .remove_pattern(pattern_set, &offset_slot, PatternId(*remove_pattern));
        }

        let possible_offset_patterns = self.wave.get_slot(&offset_slot);

        !possible_offset_patterns.is_empty()
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
