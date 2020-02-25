//! Utilities for using images. Mostly for testing the algorithms on 2D images.

use crate::{
    pattern::{PatternId, PatternMap, PatternRepresentatives, PatternSet},
    CliError, FrameConsumer,
};

use ilattice3 as lat;
use ilattice3::{Indexer, Lattice, StatelessIndexer, Tile, VoxColor, EMPTY_VOX_COLOR};
use image::{self, gif, Delay, Frame, Rgba, RgbaImage};
use std::fs::File;
use std::path::PathBuf;

pub fn make_palette_lattice<I: Indexer + Copy>(
    source_lattice: &Lattice<Rgba<u8>, I>,
    representatives: &PatternRepresentatives,
) -> Lattice<Rgba<u8>> {
    let pattern_size = *representatives.get(PatternId(0)).get_local_supremum();
    let mut palette_size = pattern_size;
    palette_size.x = (pattern_size.x + 1) * representatives.num_elements() as i32;
    let palette_extent = lat::Extent::from_min_and_local_supremum([0, 0, 0].into(), palette_size);
    let mut palette_lattice = Lattice::fill(palette_extent, Rgba([0; 4]));
    let mut next_min = [0, 0, 0].into();
    for (_, extent) in representatives.iter() {
        Lattice::copy_extent_to_position(source_lattice, &mut palette_lattice, &next_min, extent);
        next_min.x = next_min.x + pattern_size.x + 1;
    }

    palette_lattice
}

pub fn color_superposition<I: StatelessIndexer>(
    pattern_lattice: &Lattice<PatternSet>,
    tiles: &PatternMap<Tile<Rgba<u8>, I>>,
    tile_size: &lat::Point,
) -> Lattice<Rgba<u8>> {
    let full_size = *pattern_lattice.get_extent().get_local_supremum() * *tile_size;
    let full_extent = lat::Extent::from_min_and_local_supremum([0, 0, 0].into(), full_size);

    let mut color_lattice = Lattice::fill(full_extent, Rgba([0; 4]));
    for pattern_p in &pattern_lattice.get_extent() {
        let output_extent =
            lat::Extent::from_min_and_local_supremum(pattern_p * *tile_size, *tile_size);
        for p in output_extent {
            let mut num_patterns = 0;
            let patterns = pattern_lattice.get_world(&pattern_p);
            let mut color_sum = [0.0; 4];
            for pattern in patterns.iter() {
                num_patterns += 1;
                let tile: Tile<_, _> = tiles.get(pattern).clone();
                let tile = tile.put_in_extent(I::new(), output_extent);
                let Rgba(p_color) = *tile.get_world(&p);
                for i in 0..4 {
                    color_sum[i] += p_color[i] as f32;
                }
            }
            let mut mean_color = [0; 4];
            for i in 0..4 {
                mean_color[i] = (color_sum[i] / num_patterns as f32).floor() as u8;
            }
            *color_lattice.get_mut_local(&p) = Rgba(mean_color);
        }
    }

    color_lattice
}

fn color_final_patterns<C, I: StatelessIndexer>(
    pattern_lattice: &Lattice<PatternId>,
    tiles: &PatternMap<Tile<C, I>>,
    tile_size: &lat::Point,
    fill_value: C,
) -> Lattice<C>
where
    C: Clone,
{
    let full_size = *pattern_lattice.get_extent().get_local_supremum() * *tile_size;
    let full_extent = lat::Extent::from_min_and_local_supremum([0, 0, 0].into(), full_size);

    let mut color_lattice = Lattice::fill(full_extent, fill_value);
    for p in &pattern_lattice.get_extent() {
        let output_extent = lat::Extent::from_min_and_local_supremum(p * *tile_size, *tile_size);
        let pattern = pattern_lattice.get_world(&p);
        let tile = tiles
            .get(*pattern)
            .clone()
            .put_in_extent(I::new(), output_extent);
        Lattice::copy_extent(&tile, &mut color_lattice, &output_extent);
    }

    color_lattice
}

pub fn color_final_patterns_rgba<I: StatelessIndexer>(
    pattern_lattice: &Lattice<PatternId>,
    tiles: &PatternMap<Tile<Rgba<u8>, I>>,
    tile_size: &lat::Point,
) -> Lattice<Rgba<u8>> {
    color_final_patterns(pattern_lattice, tiles, tile_size, Rgba([0; 4]))
}

pub fn color_final_patterns_vox<I: StatelessIndexer>(
    pattern_lattice: &Lattice<PatternId>,
    tiles: &PatternMap<Tile<VoxColor, I>>,
    tile_size: &lat::Point,
) -> Lattice<VoxColor> {
    color_final_patterns(pattern_lattice, tiles, tile_size, EMPTY_VOX_COLOR)
}

pub struct GifMaker<I> {
    path: PathBuf,
    pattern_tiles: PatternMap<Tile<Rgba<u8>, I>>,
    tile_size: lat::Point,
    frames: Vec<Frame>,
    num_updates: usize,
    skip_frames: usize,
}

impl<I: StatelessIndexer> FrameConsumer for GifMaker<I> {
    fn use_frame(&mut self, slots: &Lattice<PatternSet>) {
        if self.num_updates % self.skip_frames == 0 {
            let superposition = color_superposition(slots, &self.pattern_tiles, &self.tile_size);
            let superposition_img: RgbaImage = (&superposition).into();
            self.frames.push(Frame::from_parts(
                superposition_img,
                0,
                0,
                Delay::from_numer_denom_ms(1, 1),
            ));
        }
        self.num_updates += 1;
    }
}

impl<I: StatelessIndexer> GifMaker<I> {
    pub fn new(
        path: PathBuf,
        pattern_tiles: PatternMap<Tile<Rgba<u8>, I>>,
        tile_size: lat::Point,
        skip_frames: usize,
    ) -> Self {
        GifMaker {
            path,
            pattern_tiles,
            tile_size,
            frames: Vec::new(),
            num_updates: 0,
            skip_frames,
        }
    }

    pub fn save(self) -> Result<(), CliError> {
        println!("Writing {:?}", self.path);
        let file_out = File::create(&self.path)?;

        gif::Encoder::new(file_out).encode_frames(self.frames.into_iter())?;

        Ok(())
    }
}
