use crate::{
    offset::OffsetId,
    pattern::{PatternGroup, PatternId, PatternMap, PatternSet, PatternSupport},
};

use ilattice3 as lat;
use ilattice3::Lattice;
use log::{debug, trace, warn};
use rand::prelude::*;

/// The colloquial "wave function" to be collapsed. Stores the possible remaining patterns that
/// could go in each slot of the output, as well as related acceleration data structures.
pub struct Wave {
    /// Sum of the possible patterns in each slot.
    collapsed_count: usize,

    /// The set of possible patterns at each slot.
    slots: Lattice<PatternSet>,

    /// The current entropy of each slot. It's faster to store this than recompute every frame.
    entropy_cache: Lattice<SlotEntropyCache>,

    /// Counts each pattern's remaining support at each offset. Once a given pattern P, for any
    /// offset, has no supporting patterns at that offset, P is no longer possible.
    pattern_supports: Lattice<PatternMap<PatternSupport>>,

    /// Container of patterns remove from slots. Currently used as a stack, but could eventually be
    /// used as a log for backtracking.
    removal_stack: Vec<(SlotId, PatternId)>,
}

impl Wave {
    pub fn new(pattern_group: &PatternGroup, output_size: lat::Point) -> Self {
        // Start with all possible patterns.
        let all_possible = PatternSet::all(pattern_group.num_patterns());

        let extent = lat::Extent::from_min_and_world_supremum([0, 0, 0].into(), output_size);
        let slots = Lattice::fill(extent, all_possible.clone());

        let initial_entropy = slot_entropy(pattern_group, &all_possible);
        debug!("Initial entropy = {:?}", initial_entropy);
        let entropy_cache = Lattice::fill(extent, initial_entropy);

        let initial_supports = pattern_group.get_initial_support();
        let pattern_supports = Lattice::fill(extent, initial_supports);

        Wave {
            slots,
            collapsed_count: 0,
            entropy_cache,
            pattern_supports,
            removal_stack: Vec::new(),
        }
    }

    pub fn num_slots(&self) -> usize {
        self.slots.get_extent().volume()
    }

    pub fn num_collapsed(&self) -> usize {
        self.collapsed_count
    }

    pub fn determined(&self) -> bool {
        self.collapsed_count == self.num_slots()
    }

    pub fn choose_least_entropy_slot<R: Rng>(&self, rng: &mut R) -> (lat::Point, f32) {
        // Micro-optimization: Don't use the extent iterator, just linear indices. It's involves far
        // less arithmetic and branching.
        (0..self.num_slots())
            .map(|linear_index| {
                let noise: f32 = rng.gen();
                let cache = *self.entropy_cache.get_linear(linear_index);
                let entropy = cache.entropy + 0.001 * noise;

                (linear_index, entropy)
            })
            .min_by(|(_, e1), (_, e2)| e1.partial_cmp(&e2).expect("Unexpected NaN"))
            .map(|(i, e)| (self.entropy_cache.local_point_from_index(i), e))
            .unwrap()
    }

    /// Forces `slot` to conform to a single pattern P. P is chosen by sampling from the prior
    /// distribution.
    pub fn observe_slot<R: Rng>(
        &mut self,
        rng: &mut R,
        pattern_group: &PatternGroup,
        slot: &lat::Point,
    ) -> bool {
        let possible_patterns = self.get_slot(slot);
        let pattern = pattern_group.sample_pattern(possible_patterns, rng);
        debug!("Assigning {:?}", pattern);

        self.collapse_slot(pattern_group, slot, pattern);

        self.propagate_constraints(&pattern_group)
    }

    /// Returns `false` iff we find a slot with no possible patterns.
    fn propagate_constraints(
        &mut self,
        pattern_group: &PatternGroup,
    ) -> bool {
        // This algorithm is similar to flood fill, but each slot may need to be visited multiple
        // times.
        while !self.removal_stack.is_empty() {
            // We know that this pattern is not longer possible at `visit_slot`, so no adjacent
            // patterns can use it as support.
            let (visit_slot, impossible_at_visit_slot) = self.removal_stack.pop().unwrap();
            let visit_slot = self.slots.local_point_from_index(visit_slot.0);
            trace!(
                "Visiting {} that removed {:?}",
                visit_slot,
                impossible_at_visit_slot
            );

            for (offset_id, offset) in pattern_group.get_offset_group().iter() {
                // Make sure we don't index out of bounds.
                // TODO: for PeriodicLatticeIndexer, don't worry about this
                let offset_slot = visit_slot + *offset;
                if !self.get_slots().get_extent().contains_world(&offset_slot) {
                    continue;
                }

                // Remove support. We detect that a pattern is not possible in a slot if it runs out
                // of supporting adjacent patterns.
                for offset_pattern in
                    pattern_group.iter_compatible(impossible_at_visit_slot, offset_id)
                {
                    trace!(
                        "Removing support for {:?} @ {}",
                        offset_pattern,
                        offset_slot
                    );
                    let no_support = self.remove_support(&offset_slot, offset_pattern, offset_id);
                    if no_support {
                        trace!("No support remaining");
                        let slot_empty =
                            self.remove_pattern(pattern_group, &offset_slot, offset_pattern);
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

    /// Returns `true` iff the slot is empty after removal.
    fn remove_pattern(
        &mut self,
        pattern_group: &PatternGroup,
        slot: &lat::Point,
        pattern: PatternId,
    ) -> bool {
        trace!("Removing {:?} from {}", pattern, slot);

        let possible_slot_patterns = self.slots.get_mut_world(slot);
        possible_slot_patterns.remove(pattern);

        let num_remaining_patterns_in_slot = possible_slot_patterns.len();
        if num_remaining_patterns_in_slot == 0 {
            return true;
        }
        if num_remaining_patterns_in_slot == 1 {
            // Don't want to choose this slot again.
            self.set_max_entropy(slot);
            self.collapsed_count += 1;
        } else {
            self.reduce_entropy(pattern_group, slot, pattern);
        }

        // Even though this pattern is being removed, it may still have support at some offsets.
        // Just clear that support now so we don't trigger another removal.
        let support = self.pattern_supports.get_mut_world(slot).get_mut(pattern);
        support.clear();

        self.removal_stack.push((SlotId(self.slots.index_from_local_point(slot)), pattern));

        false
    }

    fn collapse_slot(
        &mut self,
        pattern_group: &PatternGroup,
        slot: &lat::Point,
        assign_pattern: PatternId,
    ) -> Vec<PatternId> {
        let remove_patterns: Vec<PatternId> = {
            let set = self.slots.get_mut_world(slot);

            set.iter().filter(|p| *p != assign_pattern).collect()
        };
        for pattern in remove_patterns.iter() {
            self.remove_pattern(pattern_group, slot, *pattern);
        }

        remove_patterns
    }

    fn reduce_entropy(
        &mut self,
        pattern_group: &PatternGroup,
        slot: &lat::Point,
        remove_pattern: PatternId,
    ) {
        let cache = self.entropy_cache.get_mut_world(slot);
        let weight = pattern_group.get_weight(remove_pattern) as f32;
        cache.sum_weights -= weight;
        cache.sum_weights_log_weights -= weight * weight.log2();
        cache.entropy = entropy(cache.sum_weights, cache.sum_weights_log_weights);
    }

    fn set_max_entropy(&mut self, slot: &lat::Point) {
        let cache = self.entropy_cache.get_mut_world(slot);
        let inf = std::f32::INFINITY;
        cache.sum_weights = inf;
        cache.sum_weights_log_weights = inf;
        cache.entropy = inf;
    }

    pub fn get_slots(&self) -> &Lattice<PatternSet> {
        &self.slots
    }

    fn get_slot(&self, slot: &lat::Point) -> &PatternSet {
        self.slots.get_world(slot)
    }

    fn remove_support(&mut self, slot: &lat::Point, pattern: PatternId, offset: OffsetId) -> bool {
        self.pattern_supports
            .get_mut_world(slot)
            .get_mut(pattern)
            .remove(offset)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct SlotEntropyCache {
    sum_weights: f32,
    sum_weights_log_weights: f32,
    entropy: f32,
}

fn entropy(sum_weights: f32, sum_weights_log_weights: f32) -> f32 {
    // This is in fact a correct entropy formula, but it takes some algebra to see that it is
    // equivalent to -Σ p*log(p) where p(n) = weight(n) / Σ weight(n).
    sum_weights.log2() - sum_weights_log_weights / sum_weights
}

fn slot_entropy(pattern_group: &PatternGroup, possible_patterns: &PatternSet) -> SlotEntropyCache {
    assert!(!possible_patterns.is_empty());

    // Collapsed slots shouldn't be chosen.
    if possible_patterns.len() == 1 {
        let inf = std::f32::INFINITY;
        return SlotEntropyCache {
            sum_weights: inf,
            sum_weights_log_weights: inf,
            entropy: inf,
        };
    }

    let mut sum_weights = 0.0;
    let mut sum_weights_log_weights = 0.0;
    for pattern in possible_patterns.iter() {
        let weight = pattern_group.get_weight(pattern) as f32;
        sum_weights += weight;
        sum_weights_log_weights += weight * weight.log2();
    }
    let entropy = entropy(sum_weights, sum_weights_log_weights);

    SlotEntropyCache {
        sum_weights,
        sum_weights_log_weights,
        entropy,
    }
}

/// Linear index of a slot in the wave lattice.
struct SlotId(usize);
