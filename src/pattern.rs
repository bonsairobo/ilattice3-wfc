use crate::{
    offset::{OffsetGroup, OffsetId, OffsetMap},
    static_vec::{Id, StaticVec},
};

use hibitset::{BitSet, BitSetLike};
use ilattice3 as lat;
use ilattice3::{Indexer, Lattice, PeriodicYLevelsIndexer, Tile};
use rand::prelude::*;
use rand_distr::weighted::WeightedIndex;
use std::collections::HashMap;
use std::hash::Hash;

pub struct PatternShape {
    pub size: lat::Point,
    pub offset_group: OffsetGroup,
}

pub struct PatternSampler {
    /// Count of each pattern in the source lattice. Equivalently, a prior distribution of patterns.
    weights: PatternMap<u32>,
}

impl PatternSampler {
    pub fn new(weights: PatternMap<u32>) -> Self {
        PatternSampler { weights }
    }

    /// Returns the number of occurences of `pattern` in the source data.
    pub fn get_weight(&self, pattern: PatternId) -> u32 {
        *self.weights.get(pattern)
    }

    pub fn num_patterns(&self) -> u16 {
        self.weights.num_elements() as u16
    }

    /// Sample the possible patterns by their probability (weights) in the source data.
    pub fn sample_pattern<R: Rng>(&self, possible_patterns: &PatternSet, rng: &mut R) -> PatternId {
        let mut possible_weights = Vec::new();
        let mut possible_patterns_vec = Vec::new();
        for pattern in possible_patterns.iter() {
            possible_weights.push(*self.weights.get(pattern));
            possible_patterns_vec.push(pattern);
        }
        let dist = WeightedIndex::new(&possible_weights).unwrap();
        let choice = dist.sample(rng);

        possible_patterns_vec[choice]
    }
}

/// Represents one of the possible patterns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatternId(pub u16);

/// Limited by the support counts, which use i16.
pub const MAX_PATTERNS: u16 = std::i16::MAX as u16;

impl Into<usize> for PatternId {
    fn into(self) -> usize {
        self.0 as usize
    }
}

impl From<usize> for PatternId {
    fn from(other: usize) -> PatternId {
        PatternId(other as u16)
    }
}

impl Id for PatternId {}

const EMPTY_PATTERN_ID: PatternId = PatternId(std::u16::MAX);

/// For each unique (up to translation) sublattice of `input_lattice`, create a `PatternId`, count
/// the occurences of the pattern, and record the set of patterns that overlap with that pattern at
/// each possible offset.
pub fn process_patterns_in_lattice<T>(
    input_lattice: &Lattice<T, PeriodicYLevelsIndexer>,
    tile_size: &lat::Point,
    pattern_shape: &PatternShape,
) -> (PatternSampler, PatternConstraints, PatternRepresentatives)
where
    T: Clone + Copy + std::fmt::Debug + Eq + Hash,
{
    let full_extent = input_lattice.get_extent();
    let tiled_size = full_extent.get_local_supremum().div_ceil(tile_size);
    let tiled_extent = lat::Extent::from_min_and_local_supremum([0, 0, 0].into(), tiled_size);
    let pattern_size = pattern_shape.size * *tile_size;

    // Map sublattice data to pattern ID.
    let mut patterns: HashMap<Vec<T>, PatternId> = HashMap::new();
    // Map pattern center to pattern ID.
    let mut pattern_lattice = Lattice::<_, PeriodicYLevelsIndexer>::fill(
        tiled_extent, EMPTY_PATTERN_ID
    );
    // Map from pattern ID to sublattice.
    let mut pattern_representatives = Vec::new();
    // Map from pattern ID to # of occurrences.
    let mut pattern_weights = Vec::new();

    // Index the patterns.
    let mut next_pattern_id = 0;
    for pattern_point in tiled_extent.into_iter() {
        // Identify the pattern with the serialized values.
        let pattern_extent =
            lat::Extent::from_min_and_local_supremum(pattern_point * *tile_size, pattern_size);
        let pattern_values = input_lattice.serialize_extent(&pattern_extent);
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
    if num_patterns > MAX_PATTERNS as usize {
        panic!(
            "Too many patterns ({}), maximum is {}",
            num_patterns, MAX_PATTERNS
        );
    }
    let mut constraints =
        PatternConstraints::new(pattern_shape.offset_group.clone(), num_patterns as u16);
    for pattern_point in tiled_extent.into_iter() {
        let pattern = *pattern_lattice.get_local(&pattern_point);
        debug_assert!(pattern != EMPTY_PATTERN_ID);
        for (_, offset) in pattern_shape.offset_group.iter() {
            let offset_point = pattern_point + *offset;
            let offset_pattern = *pattern_lattice.get_local(&offset_point);
            debug_assert!(offset_pattern != EMPTY_PATTERN_ID);

            constraints.add_compatible_patterns(&offset, pattern, offset_pattern);
        }
        *pattern_weights.get_mut(pattern) += 1;
    }

    constraints.assert_valid();

    let mut sorted_weights = pattern_weights.get_raw().clone();
    sorted_weights.sort();
    println!("Weights = {:?}", sorted_weights);

    (
        PatternSampler::new(pattern_weights),
        constraints,
        PatternRepresentatives::new(pattern_representatives),
    )
}

pub fn find_pattern_tiles_in_lattice<I: Indexer, C: Clone, G: Clone + Into<C>>(
    lattice: &Lattice<G, I>,
    representatives: &PatternRepresentatives,
    tile_size: &lat::Point,
) -> PatternMap<Tile<C, I>> {
    let tiles = representatives
        .iter()
        .map(|(_, extent)| {
            // Representatives track the entire pattern, but we only need the tile where the pattern
            // is.
            let tile_extent =
                lat::Extent::from_min_and_local_supremum(extent.get_minimum(), *tile_size);

            Tile::get_from_lattice(lattice, &tile_extent)
        })
        .collect();

    PatternMap::new(tiles)
}

pub type PatternRepresentatives = PatternMap<lat::Extent>;

/// Used to build the set of pattern relations. Enforces symmetry of the `compatible` relation.
pub struct PatternConstraints {
    constraints: PatternMap<OffsetMap<BitSet>>,
    offset_group: OffsetGroup,
    num_patterns: u16,
}

impl PatternConstraints {
    pub fn new(offset_group: OffsetGroup, num_patterns: u16) -> Self {
        Self {
            constraints: PatternMap::fill(
                OffsetMap::fill(BitSet::new(), offset_group.num_offsets()),
                num_patterns as usize,
            ),
            offset_group,
            num_patterns,
        }
    }

    pub fn get_offset_group(&self) -> &OffsetGroup {
        &self.offset_group
    }

    fn assert_valid(&self) {
        for (_, c) in self.constraints.iter() {
            for i in 0..self.offset_group.num_offsets() {
                assert!(!c.get(OffsetId(i)).is_empty());
            }
        }
    }

    pub fn num_patterns(&self) -> u16 {
        self.num_patterns
    }

    pub fn iter_compatible(
        &self,
        pattern: PatternId,
        offset: OffsetId,
    ) -> impl Iterator<Item = PatternId> + '_ {
        self.constraints
            .get(pattern)
            .get(offset)
            .iter()
            .map(|i| PatternId(i as u16))
    }

    pub fn are_compatible(
        &self,
        pattern: PatternId,
        offset_pattern: PatternId,
        offset: OffsetId,
    ) -> bool {
        self.constraints
            .get(pattern)
            .get(offset)
            .contains(offset_pattern.0 as u32)
    }

    pub fn num_compatible(&self, pattern: PatternId, offset: OffsetId) -> u16 {
        self.iter_compatible(pattern, offset).count() as u16
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
            .add(offset_pattern.0 as u32);

        let opposite_id = self.offset_group.offset_id(&-*offset);
        self.constraints
            .get_mut(offset_pattern)
            .get_mut(opposite_id)
            .add(pattern.0 as u32);
    }

    /// For a fully undetermined `Wave`, return the support map for one slot.
    pub fn get_initial_support(&self) -> PatternMap<PatternSupport> {
        let mut pattern_supports = PatternMap::fill(
            PatternSupport {
                counts: OffsetMap::fill(0, self.offset_group.num_offsets()),
            },
            self.num_patterns() as usize,
        );
        for pattern in (0..self.num_patterns()).map(PatternId) {
            for offset in (0..self.offset_group.num_offsets()).map(OffsetId) {
                // If P1 allows P2 to be at offset, then P2 allows P1 to be at -offset.
                *pattern_supports.get_mut(pattern).counts.get_mut(offset) =
                    self.num_compatible(pattern, self.offset_group.opposite(offset)) as i16;
            }
        }

        pattern_supports
    }

    pub fn assignment_is_valid<I: Indexer>(&self, assignment: &Lattice<PatternId, I>) -> bool {
        let extent = assignment.get_extent();
        for p in extent {
            let pattern = assignment.get_world(&p);
            for (offset_id, offset) in self.offset_group.iter() {
                let offset_p = p + *offset;
                if !extent.contains_world(&offset_p) {
                    continue;
                }
                let offset_pattern = assignment.get_world(&offset_p);
                if !self.are_compatible(*pattern, *offset_pattern, offset_id) {
                    return false;
                }
            }
        }

        true
    }
}

/// A dynamic structure that tracks, for a pattern P (in some slot), how many patterns are
/// compatible with P at each offset. Once no patterns are compatible with P at some offset, P
/// is not possible.
#[derive(Clone)]
pub struct PatternSupport {
    counts: OffsetMap<i16>,
}

impl PatternSupport {
    /// Returns `true` iff `pattern` no longer gives any support.
    pub fn remove(&mut self, offset: OffsetId) -> bool {
        let count = self.counts.get_mut(offset);
        *count -= 1;

        *count == 0
    }

    pub fn clear(&mut self) {
        self.counts
            .iter_mut()
            .for_each(|(_offset, count)| *count = 0);
    }
}

pub type PatternMap<T> = StaticVec<PatternId, T>;

#[derive(Clone)]
pub struct PatternSet {
    bits: BitSet,
    size: u16,
}

impl PatternSet {
    pub fn all(num_patterns: u16) -> Self {
        let mut bits = BitSet::with_capacity(num_patterns as u32);
        for i in 0..num_patterns {
            bits.add(i as u32);
        }

        PatternSet {
            size: num_patterns,
            bits,
        }
    }

    pub fn len(&self) -> u16 {
        self.size
    }

    pub fn remove(&mut self, pattern: PatternId) {
        self.bits.remove(pattern.0 as u32);
        self.size -= 1;
    }

    pub fn iter(&self) -> impl Iterator<Item = PatternId> + '_ {
        (&self.bits).iter().map(|i| PatternId(i as u16))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
