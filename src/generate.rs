use crate::{
    offset::OffsetId,
    pattern::{PatternId, PatternGroup},
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
        &self.wave.slots
    }

    /// Warning: undefined behavior if called before `update` returns `Success`.
    pub fn result(&self) -> Lattice<PatternId> {
        self.wave.slots.map(|possible_patterns: &BitSet| {
            let only_pattern: u32 = possible_patterns.iter().next().unwrap();

            PatternId(only_pattern)
        })
    }

    pub fn update(&mut self, patterns: &PatternGroup) -> UpdateResult {
        let (slot, entropy) = { self.wave.choose_lowest_entropy_slot(&mut self.rng) };
        debug!(
            "{} candidate patterns remaining; chose slot {} with entropy {}",
            self.wave.remaining_pattern_count, slot, entropy
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
        let possible_patterns = self.wave.slots.get_world(slot);
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
                if !self.wave.slots.get_extent().contains_world(&offset_slot) {
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
        let possible_offset_patterns = self.wave.slots.get_world(&offset_slot);
        for offset_pattern in possible_offset_patterns.iter() {
            // `pattern` is not possible if it's not compatible with any of the possibilities at
            // `visit_slot`.
            let possible_visit_slot_patterns = self.wave.slots.get_world(&visit_slot);
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

        let possible_offset_patterns = self.wave.slots.get_world(&offset_slot);

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

/// The possible remaining patterns that could go in each slot of the output. The colloquial "wave
/// function" to be collapsed.
struct Wave {
    slots: Lattice<BitSet>,
    entropy_cache: Lattice<SlotEntropyCache>,
    remaining_pattern_count: u32,
}

impl Wave {
    fn new(pattern_group: &PatternGroup, output_size: lat::Point) -> Self {
        // Start with all possible patterns.
        let mut all_possible = BitSet::with_capacity(pattern_group.num_patterns());
        for i in 0..pattern_group.num_patterns() {
            all_possible.add(i);
        }

        let extent = lat::Extent::from_min_and_world_supremum([0, 0, 0].into(), output_size);
        let slots = Lattice::fill(extent, all_possible.clone());
        let remaining_pattern_count =
            slots.get_extent().volume() as u32 * pattern_group.num_patterns();

        let initial_entropy = slot_entropy(pattern_group, &all_possible);
        debug!("Initial entropy = {:?}", initial_entropy);
        let entropy_cache = Lattice::fill(extent, initial_entropy);

        Wave {
            slots,
            remaining_pattern_count,
            entropy_cache,
        }
    }

    fn determined(&self) -> bool {
        self.remaining_pattern_count == self.slots.get_extent().volume() as u32
    }

    fn choose_lowest_entropy_slot<R: Rng>(&self, rng: &mut R) -> (lat::Point, f32) {
        self.entropy_cache
            .get_extent()
            .into_iter()
            .map(|s| {
                let noise: f32 = rng.gen();
                let cache = *self.entropy_cache.get_world(&s);
                let entropy = cache.entropy + 0.001 * noise;

                (s, entropy)
            })
            .min_by(|(_, e1), (_, e2)| e1.partial_cmp(&e2).expect("Unexpected NaN"))
            .unwrap()
    }

    fn remove_pattern(
        &mut self, pattern_group: &PatternGroup, slot: &lat::Point, pattern: PatternId
    ) {
        let possible_offset_patterns = self.slots.get_mut_world(slot);
        possible_offset_patterns.remove(pattern.0);
        if possible_offset_patterns.iter().count() == 1 {
            // Don't want to choose this slot again.
            self.set_max_entropy(slot);
        } else {
            self.lower_entropy(pattern_group, slot, pattern);
        }
        self.remaining_pattern_count -= 1;
    }

    fn collapse_slot(
        &mut self,
        pattern_group: &PatternGroup,
        slot: &lat::Point,
        assign_pattern: PatternId,
    ) {
        let remove_patterns: Vec<u32> = {
            let set = self.slots.get_mut_world(slot);

            set.iter().filter(|p| *p != assign_pattern.0).collect()
        };
        for removed_pattern in remove_patterns.into_iter() {
            self.remove_pattern(pattern_group, slot, PatternId(removed_pattern));
        }
    }

    fn lower_entropy(
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

fn slot_entropy(pattern_group: &PatternGroup, possible_patterns: &BitSet) -> SlotEntropyCache {
    assert!(!possible_patterns.is_empty());

    // Collapsed slots shouldn't be chosen.
    if possible_patterns.iter().count() == 1 {
        let inf = std::f32::INFINITY;
        return SlotEntropyCache {
            sum_weights: inf,
            sum_weights_log_weights: inf,
            entropy: inf,
        };
    }

    let mut sum_weights = 0.0;
    let mut sum_weights_log_weights = 0.0;
    for id in possible_patterns.iter() {
        let weight = pattern_group.get_weight(PatternId(id)) as f32;
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
