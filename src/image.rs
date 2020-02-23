//! Utilities for using images. Mostly for testing the algorithms on 2D images.

use crate::{
    pattern::{PatternId, PatternMap, PatternRepresentatives, PatternSet, Tile},
    CliError, FrameConsumer,
};

use ilattice3 as lat;
use ilattice3::{Lattice, LatticeIndexer, VoxColor, EMPTY_VOX_COLOR};
use image::{self, gif, Delay, Frame, Rgba, RgbaImage};
use std::fs::File;
use std::path::PathBuf;

// TODO: conversions should go in a new ilattice3 image feature

pub fn lattice_from_image<I: LatticeIndexer>(indexer: I, img: &RgbaImage) -> Lattice<Rgba<u8>, I> {
    let size = [img.width() as i32, img.height() as i32, 1].into();
    let extent = lat::Extent::from_min_and_local_supremum([0, 0, 0].into(), size);
    let mut lattice = Lattice::fill_with_indexer(indexer, extent, Rgba([0; 4]));
    for (x, y, pixel) in img.enumerate_pixels() {
        let point = [x as i32, y as i32, 0].into();
        *lattice.get_mut_local(&point) = *pixel;
    }

    lattice
}

pub fn image_from_lattice<I: LatticeIndexer>(lattice: &Lattice<Rgba<u8>, I>) -> RgbaImage {
    let extent = lattice.get_extent();
    let size = extent.get_local_supremum();
    debug_assert_eq!(size.z, 1);
    debug_assert!(size.x > 0);
    debug_assert!(size.y > 0);
    let (width, height) = (size.x as u32, size.y as u32);

    let mut img = RgbaImage::new(width, height);
    for p in extent {
        *img.get_pixel_mut(p.x as u32, p.y as u32) = *lattice.get_local(&p);
    }

    img
}

pub fn make_palette_lattice<I: LatticeIndexer + Copy>(
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

pub fn color_superposition(
    pattern_lattice: &Lattice<PatternSet>,
    tiles: &PatternMap<Tile<Rgba<u8>>>,
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
                let tile = tiles.get(pattern).put_in_extent(&output_extent);
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

fn color_final_patterns<C>(
    pattern_lattice: &Lattice<PatternId>,
    tiles: &PatternMap<Tile<C>>,
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
        let tile = tiles.get(*pattern).put_in_extent(&output_extent);
        Lattice::copy_extent(&tile, &mut color_lattice, &output_extent);
    }

    color_lattice
}

pub fn color_final_patterns_rgba(
    pattern_lattice: &Lattice<PatternId>,
    tiles: &PatternMap<Tile<Rgba<u8>>>,
    tile_size: &lat::Point,
) -> Lattice<Rgba<u8>> {
    color_final_patterns(pattern_lattice, tiles, tile_size, Rgba([0; 4]))
}

pub fn color_final_patterns_vox(
    pattern_lattice: &Lattice<PatternId>,
    tiles: &PatternMap<Tile<VoxColor>>,
    tile_size: &lat::Point,
) -> Lattice<VoxColor> {
    color_final_patterns(pattern_lattice, tiles, tile_size, EMPTY_VOX_COLOR)
}

pub struct GifMaker {
    path: PathBuf,
    pattern_tiles: PatternMap<Tile<Rgba<u8>>>,
    tile_size: lat::Point,
    frames: Vec<Frame>,
    num_updates: usize,
    skip_frames: usize,
}

impl FrameConsumer for GifMaker {
    fn use_frame(&mut self, slots: &Lattice<PatternSet>) {
        if self.num_updates % self.skip_frames == 0 {
            let superposition = color_superposition(slots, &self.pattern_tiles, &self.tile_size);
            let superposition_img = image_from_lattice(&superposition);
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

impl GifMaker {
    pub fn new(
        path: PathBuf,
        pattern_tiles: PatternMap<Tile<Rgba<u8>>>,
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
