use crate::{
    offset::{OffsetGroup, OffsetId, OffsetMap},
    static_vec::{Id, StaticVec},
};

use hibitset::{BitSet, BitSetLike};
use ilattice3 as lat;
use ilattice3::{Lattice, LatticeIndexer, PeriodicYLevelsIndexer};
use rand::prelude::*;
use rand_distr::weighted::WeightedIndex;
use std::collections::HashMap;
use std::hash::Hash;

pub struct PatternShape {
    pub size: lat::Point,
    pub offset_group: OffsetGroup,
}

/// Metadata about configurations of voxels, called "patterns," and how they are related.
pub struct PatternGroup {
    /// Count of each pattern in the source lattice. Equivalently, a prior distribution of patterns.
    weights: PatternMap<u32>,
    /// One set of constraints for each pattern.
    pub(crate) constraints: SymmetricPatternConstraints,
}

// TODO: reduce to u16; we should never need more than 65536 patterns
/// Represents one of the possible patterns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatternId(pub u32);

impl Into<usize> for PatternId {
    fn into(self) -> usize {
        self.0 as usize
    }
}

impl From<usize> for PatternId {
    fn from(other: usize) -> PatternId {
        PatternId(other as u32)
    }
}

impl Id for PatternId {}

const EMPTY_PATTERN_ID: PatternId = PatternId(std::u32::MAX);

impl PatternGroup {
    pub fn new(weights: PatternMap<u32>, constraints: SymmetricPatternConstraints) -> Self {
        let me = PatternGroup {
            weights,
            constraints,
        };
        me.assert_valid();

        me
    }

    pub fn assert_valid(&self) {
        assert!(self.weights.num_elements() as u32 == self.constraints.num_patterns());
        self.constraints.assert_valid();
    }

    /// Returns the number of occurences of the pattern `id` in the source data.
    pub fn get_weight(&self, id: PatternId) -> u32 {
        *self.weights.get(id)
    }

    pub fn num_patterns(&self) -> u32 {
        self.weights.num_elements() as u32
    }

    /// Sample the possible patterns by their probability (weights) in the source data.
    pub fn sample_pattern<R>(&self, possible_patterns: &BitSet, rng: &mut R) -> PatternId
    where
        R: Rng,
    {
        let mut possible_weights = Vec::new();
        let mut possible_patterns_vec = Vec::new();
        for i in possible_patterns.iter() {
            let id = PatternId(i);
            possible_weights.push(*self.weights.get(id));
            possible_patterns_vec.push(id);
        }
        let dist = WeightedIndex::new(&possible_weights).unwrap();
        let choice = dist.sample(rng);

        possible_patterns_vec[choice]
    }
}

// TODO: support non-periodic indexer
/// For each unique (up to translation) sublattice of `lattice`, create a `PatternId`, count the
/// occurences of the pattern, and record the set of patterns that overlap with that pattern at each
/// possible offset.
pub fn process_patterns_in_lattice<T>(
    lattice: &Lattice<T, PeriodicYLevelsIndexer>,
    pattern_shape: &PatternShape,
) -> (PatternGroup, PatternRepresentatives)
where
    T: Clone + Copy + Default + Eq + Hash,
{
    let full_extent = lattice.get_extent();

    // Map sublattice data to pattern ID.
    let mut patterns: HashMap<Vec<T>, PatternId> = HashMap::new();
    // Map pattern center to pattern ID.
    let mut pattern_lattice =
        Lattice::fill_with_indexer(lattice.indexer, full_extent, EMPTY_PATTERN_ID);
    // Map from pattern ID to sublattice.
    let mut pattern_representatives = Vec::new();
    // Map from pattern ID to # of occurrences.
    let mut pattern_weights = Vec::new();

    // Index the patterns.
    let mut next_pattern_id = 0;
    for pattern_point in full_extent.into_iter() {
        // Identify the pattern with the serialized values.
        let pattern_extent =
            lat::Extent::from_min_and_local_supremum(pattern_point, pattern_shape.size);
        let pattern_values = lattice.serialize_extent(&pattern_extent);
        let pattern_id = patterns.entry(pattern_values).or_insert_with(|| {
            let this_pattern_id = PatternId(next_pattern_id);
            next_pattern_id += 1;
            pattern_representatives.push(pattern_extent);
            pattern_weights.push(0);

            this_pattern_id
        });
        *pattern_lattice.get_mut_local(&pattern_point) = *pattern_id;
    }

    let mut pattern_weights = PatternMap::new(pattern_weights);

    // Set the constraints and count pattern occurences.
    let num_patterns = patterns.len();
    let mut pattern_constraints =
        SymmetricPatternConstraints::new(pattern_shape.offset_group.clone(), num_patterns as u32);
    for pattern_point in full_extent.into_iter() {
        let pattern = *pattern_lattice.get_local(&pattern_point);
        debug_assert!(pattern != EMPTY_PATTERN_ID);
        for (_, offset) in pattern_shape.offset_group.iter() {
            let offset = *offset;
            let offset_point = pattern_point + offset;
            let offset_pattern = *pattern_lattice.get_local(&offset_point);
            debug_assert!(offset_pattern != EMPTY_PATTERN_ID);

            pattern_constraints.add_compatible_patterns(&offset, pattern, offset_pattern);
            *pattern_weights.get_mut(pattern) += 1;
        }
    }

    let pattern_set = PatternGroup::new(pattern_weights, pattern_constraints);

    (
        pattern_set,
        PatternRepresentatives::new(pattern_representatives),
    )
}

pub type PatternRepresentatives = PatternMap<lat::Extent>;

/// Used to build the set of pattern relations. Enforces symmetry of the `compatible` relation.
pub struct SymmetricPatternConstraints {
    constraints: PatternMap<OffsetMap<BitSet>>,
    pub(crate) offset_group: OffsetGroup,
    num_patterns: u32,
}

impl SymmetricPatternConstraints {
    pub fn new(offset_group: OffsetGroup, num_patterns: u32) -> Self {
        Self {
            constraints: PatternMap::fill(
                OffsetMap::fill(BitSet::new(), offset_group.num_offsets()),
                num_patterns as usize,
            ),
            offset_group,
            num_patterns,
        }
    }

    fn assert_valid(&self) {
        for (_, c) in self.constraints.iter() {
            for i in 0..self.offset_group.num_offsets() {
                assert!(!c.get(OffsetId(i)).is_empty());
            }
        }
    }

    pub fn num_patterns(&self) -> u32 {
        self.num_patterns
    }

    pub fn iter_compatible(
        &self, pattern: PatternId, offset: OffsetId
    ) -> impl Iterator<Item = PatternId> + '_ {
        self.constraints.get(pattern).get(offset).iter().map(|i| PatternId(i))
    }

    pub fn num_compatible(&self, pattern: PatternId, offset: OffsetId) -> u32 {
        self.iter_compatible(pattern, offset).count() as u32
    }

    pub fn add_compatible_patterns(
        &mut self,
        offset: &lat::Point,
        pattern: PatternId,
        offset_pattern: PatternId,
    ) {
        let offset_id = self.offset_group.offset_id(offset);
        self.constraints
            .get_mut(pattern)
            .get_mut(offset_id)
            .add(offset_pattern.0);

        let opposite_id = self.offset_group.offset_id(&-*offset);
        self.constraints
            .get_mut(offset_pattern)
            .get_mut(opposite_id)
            .add(pattern.0);
    }

    /// For a fully undetermined `Wave`, return the support map for one slot.
    pub fn get_initial_support(&self) -> PatternMap<PatternSupport> {
        let mut pattern_supports = PatternMap::fill(
            PatternSupport { counts: OffsetMap::fill(0, self.offset_group.num_offsets()) },
            self.num_patterns() as usize
        );
        for pattern in (0..self.num_patterns()).map(|p| PatternId(p)) {
            for offset in (0..self.offset_group.num_offsets()).map(|o| OffsetId(o)) {
                // If P1 allows P2 to be at offset, then P2 allows P1 to be at -offset.
                *pattern_supports.get_mut(pattern).counts.get_mut(offset) =
                    self.num_compatible(pattern, self.offset_group.opposite(offset)) as i32;
            }
        }

        pattern_supports
    }
}

/// A dynamic structure that tracks, for a pattern P (in some slot), how many patterns are
/// compatible with P at each offset. Once no patterns are compatible with P at some offset, P
/// is not possible.
#[derive(Clone)]
pub struct PatternSupport {
    counts: OffsetMap<i32>,
}

impl PatternSupport {
    /// Returns `true` iff `pattern` no longer gives any support.
    pub fn remove(&mut self, offset: OffsetId) -> bool {
        let count = self.counts.get_mut(offset);
        *count -= 1;

        *count == 0
    }

    pub fn clear(&mut self) {
        self.counts = OffsetMap::fill(0, self.counts.num_elements());
    }
}

pub type PatternColors = PatternMap<[u8; 4]>;

pub fn find_pattern_colors<I: LatticeIndexer>(
    lattice: &Lattice<u32, I>,
    representatives: &PatternRepresentatives,
) -> PatternColors {
    representatives.map(|e| unsafe { std::mem::transmute(*lattice.get_local(&e.get_minimum())) })
}

pub type PatternMap<T> = StaticVec<PatternId, T>;
