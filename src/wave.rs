use crate::pattern::{PatternGroup, PatternId};

use hibitset::{BitSet, BitSetLike};
use ilattice3 as lat;
use ilattice3::Lattice;
use log::debug;
use rand::prelude::*;

/// The possible remaining patterns that could go in each slot of the output. The colloquial "wave
/// function" to be collapsed.
pub struct Wave {
    slots: Lattice<BitSet>,
    entropy_cache: Lattice<SlotEntropyCache>,
    remaining_pattern_count: u32,
}

impl Wave {
    pub fn new(pattern_group: &PatternGroup, output_size: lat::Point) -> Self {
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

    pub fn get_remaining_pattern_count(&self) -> u32 {
        self.remaining_pattern_count
    }

    pub fn determined(&self) -> bool {
        self.remaining_pattern_count == self.slots.get_extent().volume() as u32
    }

    pub fn choose_lowest_entropy_slot<R: Rng>(&self, rng: &mut R) -> (lat::Point, f32) {
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

    pub fn remove_pattern(
        &mut self, pattern_group: &PatternGroup, slot: &lat::Point, pattern: PatternId
    ) {
        let possible_offset_patterns = self.slots.get_mut_world(slot);
        possible_offset_patterns.remove(pattern.0);
        if possible_offset_patterns.iter().count() == 1 {
            // Don't want to choose this slot again.
            self.set_max_entropy(slot);
        } else {
            self.reduce_entropy(pattern_group, slot, pattern);
        }
        self.remaining_pattern_count -= 1;
    }

    pub fn collapse_slot(
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

    pub fn reduce_entropy(
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

    pub fn set_max_entropy(&mut self, slot: &lat::Point) {
        let cache = self.entropy_cache.get_mut_world(slot);
        let inf = std::f32::INFINITY;
        cache.sum_weights = inf;
        cache.sum_weights_log_weights = inf;
        cache.entropy = inf;
    }

    pub fn get_slots(&self) -> &Lattice<BitSet> {
        &self.slots
    }

    pub fn get_slot(&self, slot: &lat::Point) -> &BitSet {
        self.slots.get_world(slot)
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
