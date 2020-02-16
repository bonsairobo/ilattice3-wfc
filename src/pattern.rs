use crate::offset::{OffsetGroup, OffsetId};

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
pub struct PatternSet {
    /// Count of each pattern in the source lattice. Equivalently, a prior distribution of patterns.
    weights: PatternData<u32>,
    /// One set of constraints for each pattern.
    pub(crate) constraints: SymmetricPatternConstraints,
}

/// Represents one of the possible patterns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatternId(pub u32);

const EMPTY_PATTERN_ID: PatternId = PatternId(std::u32::MAX);

impl PatternSet {
    pub fn new(weights: PatternData<u32>, constraints: SymmetricPatternConstraints) -> Self {
        let me = PatternSet {
            weights,
            constraints,
        };
        me.assert_valid();

        me
    }

    pub fn assert_valid(&self) {
        assert!(self.weights.num_patterns() == self.constraints.num_patterns());
        self.constraints.assert_valid();
    }

    /// Returns the number of occurences of the pattern `id` in the source data.
    pub fn get_weight(&self, id: PatternId) -> u32 {
        *self.weights.get(id)
    }

    // Based on the index type of `BitSet`, we can return at most a `u32`.
    pub fn num_patterns(&self) -> u32 {
        self.weights.num_patterns()
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
) -> (PatternSet, PatternRepresentatives)
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

    let mut pattern_weights = PatternData::new(pattern_weights);

    // Set the constraints and count pattern occurences.
    let num_patterns = patterns.len();
    let mut pattern_constraints =
        SymmetricPatternConstraints::new(pattern_shape.offset_group.clone(), num_patterns);
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

    let pattern_set = PatternSet::new(pattern_weights, pattern_constraints);

    (
        pattern_set,
        PatternRepresentatives::new(pattern_representatives),
    )
}

pub type PatternRepresentatives = PatternData<lat::Extent>;

/// Enforces symmetry of the `compatible` relation.
pub struct SymmetricPatternConstraints {
    constraints: PatternData<PatternConstraints>,
    pub(crate) offset_group: OffsetGroup,
}

impl SymmetricPatternConstraints {
    pub fn new(offset_group: OffsetGroup, num_patterns: usize) -> Self {
        Self {
            constraints: PatternData::fill(
                PatternConstraints::new(offset_group.num_offsets()),
                num_patterns,
            ),
            offset_group,
        }
    }

    fn assert_valid(&self) {
        for (_, c) in self.constraints.iter() {
            for i in 0..self.offset_group.num_offsets() {
                assert!(!c.allowed_adjacent_patterns[i].is_empty());
            }
        }
    }

    pub fn num_patterns(&self) -> u32 {
        self.constraints.num_patterns()
    }

    /// Returns whether `pattern` is compatible with `offset_pattern` in the configuration where
    /// the center of `offset_pattern` is `offset` from the center of `pattern`. This should be an
    /// antisymmetric relation, i.e. `compatible(t, p1, p2) <=> compatible(-t, p2, p1)`.
    pub fn compatible(
        &self,
        offset: OffsetId,
        pattern: PatternId,
        offset_pattern: PatternId,
    ) -> bool {
        self.constraints.get(pattern).allowed_adjacent_patterns[offset.0].contains(offset_pattern.0)
    }

    pub fn add_compatible_patterns(
        &mut self,
        offset: &lat::Point,
        pattern: PatternId,
        offset_pattern: PatternId,
    ) {
        let offset_id = self.offset_group.offset_id(offset);
        self.constraints.get_mut(pattern).allowed_adjacent_patterns[offset_id.0]
            .add(offset_pattern.0);

        let opposite_id = self.offset_group.offset_id(&-*offset);
        self.constraints
            .get_mut(offset_pattern)
            .allowed_adjacent_patterns[opposite_id.0]
            .add(pattern.0);
    }
}

/// At each offset, the set of patterns that are compatible with another pattern.
#[derive(Clone, Default)]
struct PatternConstraints {
    // Stored in the same order as `OVERLAP_OFFSETS` for efficiency.
    allowed_adjacent_patterns: Vec<BitSet>,
}

impl PatternConstraints {
    fn new(num_offsets: usize) -> Self {
        PatternConstraints {
            allowed_adjacent_patterns: vec![BitSet::new(); num_offsets],
        }
    }
}

pub type PatternColors = PatternData<[u8; 4]>;

pub fn find_pattern_colors<I: LatticeIndexer>(
    lattice: &Lattice<u32, I>,
    representatives: &PatternRepresentatives,
) -> PatternColors {
    representatives.map(|e| unsafe { std::mem::transmute(*lattice.get_local(&e.get_minimum())) })
}

pub struct PatternData<T> {
    data: Vec<T>,
}

impl<T: Clone + Default> PatternData<T> {
    pub fn new_with_default(num_patterns: usize) -> Self {
        PatternData {
            data: vec![T::default(); num_patterns],
        }
    }
}

impl<T: Clone> PatternData<T> {
    pub fn fill(value: T, num_patterns: usize) -> Self {
        PatternData {
            data: vec![value; num_patterns],
        }
    }
}

impl<T> PatternData<T> {
    pub fn new(data: Vec<T>) -> Self {
        Self { data }
    }

    pub fn get(&self, pattern_id: PatternId) -> &T {
        &self.data[pattern_id.0 as usize]
    }

    pub fn get_mut(&mut self, pattern_id: PatternId) -> &mut T {
        &mut self.data[pattern_id.0 as usize]
    }

    pub fn iter(&self) -> impl Iterator<Item = (PatternId, &T)> {
        self.data
            .iter()
            .enumerate()
            .map(|(i, d)| (PatternId(i as u32), d))
    }

    pub fn map<S, F: Fn(&T) -> S>(&self, f: F) -> PatternData<S> {
        PatternData::new(self.data.iter().map(f).collect())
    }

    pub fn num_patterns(&self) -> u32 {
        self.data.len() as u32
    }
}
