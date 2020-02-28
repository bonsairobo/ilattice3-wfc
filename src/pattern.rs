use crate::{
    offset::{OffsetGroup, OffsetId, OffsetMap},
    static_vec::{Id, StaticVec},
};

use hibitset::{BitSet, BitSetLike};
use ilattice3 as lat;
use ilattice3::{
    Indexer, Lattice, PeriodicYLevelsIndexer, Tile, Transform, Z_STATIONARY_OCTAHEDRAL_GROUP
};
use rand::prelude::*;
use rand_distr::weighted::WeightedIndex;
use std::collections::{HashMap, HashSet};
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

pub fn find_unique_tiles<T>(
    input_lattice: &Lattice<T, PeriodicYLevelsIndexer>,
    tile_size: &lat::Point,
) -> TileSet<T, PeriodicYLevelsIndexer>
where
    T: Clone + Copy + std::fmt::Debug + Eq + Hash,
{
    let input_extent = input_lattice.get_extent();
    let index_extent = lat::Extent::from_min_and_local_supremum(
        [0, 0, 0].into(), input_extent.get_local_supremum().div_ceil(tile_size)
    );

    let mut tiles: HashSet<Tile<T, _>> = HashSet::new();

    for p in index_extent {
        let tile_min = p * *tile_size;
        let tile_extent = lat::Extent::from_min_and_local_supremum(tile_min, *tile_size);
        let tile_lattice = input_lattice.copy_extent_into_new_lattice(&tile_extent);

        // Identify any symmetric configurations of a tile.
        let mut add_tile = None;
        for symmetry in Z_STATIONARY_OCTAHEDRAL_GROUP.iter() {
            let transform = Transform { matrix: symmetry.clone() };
            let mut transformed_tile_lattice = tile_lattice.apply_octahedral_transform(&transform);
            transformed_tile_lattice.set_minimum(&[0, 0, 0].into()); // normalize
            let normalized_extent = lat::Extent::from_min_and_local_supremum(
                [0, 0, 0].into(), *tile_size
            );
            let transformed_tile = Tile::get_from_lattice(
                &transformed_tile_lattice, &normalized_extent
            );

            if tiles.contains(&transformed_tile) {
                add_tile = None;
                break;
            }

            add_tile = Some(transformed_tile);
        }

        if let Some(tile) = add_tile {
            tiles.insert(tile);
        }
    }

    TileSet { tiles: tiles.into_iter().collect(), tile_size: *tile_size }
}

/// For each unique (up to translation) sublattice of `input_lattice`, create a `PatternId`, count
/// the occurences of the pattern, and record the set of patterns that overlap with that pattern at
/// each possible offset.
pub fn process_patterns_in_lattice<T>(
    input_lattice: &Lattice<T, PeriodicYLevelsIndexer>,
    tile_size: &lat::Point,
    pattern_shape: &PatternShape,
) -> (PatternSampler, PatternConstraints, PatternTileSet<T, PeriodicYLevelsIndexer>)
where
    T: Clone + Copy + std::fmt::Debug + Eq + Hash,
{
    let input_extent = input_lattice.get_extent();
    let pattern_size = pattern_shape.size * *tile_size;
    let pattern_lattice_size = input_extent.get_local_supremum().div_ceil(tile_size);

    let mut num_patterns = 0;
    // Map sublattice data to pattern ID.
    let mut patterns: HashMap<Tile<T, _>, PatternId> = HashMap::new();
    // Min corner tile of each pattern.
    let mut pattern_min_tiles = Vec::new();
    // Map from pattern ID to # of occurrences.
    let mut pattern_weights = PatternMap::new(Vec::new());

    let mut constraints = PatternConstraints::new(pattern_shape.offset_group.clone());

    let pattern_lattice_extent = lat::Extent::from_min_and_local_supremum(
        [0, 0, 0].into(), pattern_lattice_size
    );

    // Map pattern center to pattern ID.
    let mut pattern_lattice = Lattice::<_, PeriodicYLevelsIndexer>::fill(
        pattern_lattice_extent, EMPTY_PATTERN_ID
    );

    // Index the patterns.
    for pattern_point in pattern_lattice_extent.into_iter() {
        // Identify the pattern with the serialized values.
        let pattern_min = pattern_point * *tile_size;
        let pattern_extent = lat::Extent::from_min_and_local_supremum(pattern_min, pattern_size);
        let tile_extent = lat::Extent::from_min_and_local_supremum(pattern_min, *tile_size);

        let pattern = Tile::get_from_lattice(input_lattice, &pattern_extent);
        let pattern_min_tile = Tile::get_from_lattice(input_lattice, &tile_extent);

        let pattern_id = patterns.entry(pattern).or_insert_with(|| {
            let this_pattern_id = PatternId(num_patterns);

            num_patterns += 1;
            if num_patterns > MAX_PATTERNS {
                panic!(
                    "Too many patterns ({}), maximum is {}",
                    num_patterns, MAX_PATTERNS
                );
            }

            constraints.add_pattern();
            pattern_weights.push(0);
            pattern_min_tiles.push(pattern_min_tile);

            this_pattern_id
        });
        *pattern_lattice.get_mut_local(&pattern_point) = *pattern_id;
    }

    // Set the constraints and count pattern occurences.
    for pattern_point in pattern_lattice_extent.into_iter() {
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
        PatternTileSet { tiles: PatternMap::new(pattern_min_tiles), tile_size: *tile_size, }
    )
}

#[derive(Clone)]
pub struct TileSet<T, I> {
    pub tiles: Vec<Tile<T, I>>,
    pub tile_size: lat::Point,
}

impl<T, I> From<TileSet<T, I>> for PatternTileSet<T, I> {
    fn from(other: TileSet<T, I>) -> Self {
        let TileSet { tiles, tile_size } = other;

        PatternTileSet {
            tiles: PatternMap::new(tiles),
            tile_size,
        }
    }
}

impl<T, I> From<PatternTileSet<T, I>> for TileSet<T, I> {
    fn from(other: PatternTileSet<T, I>) -> Self {
        let PatternTileSet { tiles, tile_size } = other;

        TileSet {
            tiles: tiles.into_raw(),
            tile_size,
        }
    }
}

#[derive(Clone)]
pub struct PatternTileSet<T, I> {
    pub tiles: PatternMap<Tile<T, I>>,
    pub tile_size: lat::Point,
}

/// Used to build the set of pattern relations. Enforces symmetry of the `compatible` relation.
pub struct PatternConstraints {
    constraints: PatternMap<OffsetMap<BitSet>>,
    offset_group: OffsetGroup,
}

impl PatternConstraints {
    pub fn new(offset_group: OffsetGroup) -> Self {
        Self {
            constraints: PatternMap::new(Vec::new()),
            offset_group,
        }
    }

    pub fn add_pattern(&mut self) {
        self.constraints.push(OffsetMap::fill(BitSet::new(), self.offset_group.num_offsets()));
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
        self.constraints.num_elements() as u16
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
