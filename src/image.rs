//! Utilities for using images. Mostly for testing the algorithms on 2D images.

use crate::pattern::{PatternColors, PatternId, PatternRepresentatives};

use hibitset::{BitSet, BitSetLike};
use ilattice3 as lat;
use ilattice3::{Lattice, LatticeIndexer};
use image::{self, Rgba, RgbaImage};

pub fn lattice_from_image<I: LatticeIndexer>(indexer: I, img: &RgbaImage) -> Lattice<u32, I> {
    let size = [img.width() as i32, img.height() as i32, 1].into();
    let extent = lat::Extent::from_min_and_local_supremum([0, 0, 0].into(), size);
    let mut lattice = Lattice::fill_with_indexer(indexer, extent, 0);
    for (x, y, pixel) in img.enumerate_pixels() {
        let point = [x as i32, y as i32, 0].into();
        *lattice.get_mut_local(&point) = integer_from_rgba(*pixel);
    }

    lattice
}

pub fn integer_from_rgba(rgba: Rgba<u8>) -> u32 {
    unsafe { std::mem::transmute(rgba) }
}

pub fn image_from_lattice<I: LatticeIndexer>(lattice: &Lattice<u32, I>) -> RgbaImage {
    let extent = lattice.get_extent();
    let size = extent.get_local_supremum();
    debug_assert_eq!(size.z, 1);
    debug_assert!(size.x > 0);
    debug_assert!(size.y > 0);
    let (width, height) = (size.x as usize, size.y as usize);

    let mut buf = vec![0; width * height * 4];
    for p in &extent {
        let (x, y) = (p.x as usize, p.y as usize);
        let rgba = rgba_from_integer(*lattice.get_local(&p));
        let i = 4 * (x + width * y);
        buf[i..(4 + i)].clone_from_slice(&rgba[..4]);
    }

    RgbaImage::from_raw(width as u32, height as u32, buf).unwrap()
}

pub fn rgba_from_integer(i: u32) -> [u8; 4] {
    unsafe { std::mem::transmute(i) }
}

pub fn make_palette_lattice<I: LatticeIndexer + Copy>(
    source_lattice: &Lattice<u32, I>,
    representatives: &PatternRepresentatives,
) -> Lattice<u32, I> {
    let pattern_size = *representatives.get(PatternId(0)).get_local_supremum();
    let mut palette_size = pattern_size;
    palette_size.x = (pattern_size.x + 1) * representatives.num_patterns() as i32;
    let palette_extent = lat::Extent::from_min_and_local_supremum([0, 0, 0].into(), palette_size);
    let mut palette_lattice = Lattice::fill_with_indexer(source_lattice.indexer, palette_extent, 0);
    let mut next_min = [0, 0, 0].into();
    for (_, extent) in representatives.iter() {
        Lattice::copy_extent_to_position(source_lattice, &mut palette_lattice, &next_min, extent);
        next_min.x = next_min.x + pattern_size.x + 1;
    }

    palette_lattice
}

pub fn color_superposition(lattice: &Lattice<BitSet>, colors: &PatternColors) -> Lattice<u32> {
    let mut color_lattice = Lattice::fill(lattice.get_extent(), 0);
    for p in &color_lattice.get_extent() {
        let mut color_sum = [0.0; 4];
        let mut num_patterns = 0;
        for pattern_id in lattice.get_local(&p).iter().map(PatternId) {
            let pattern_color = colors.get(pattern_id);
            for i in 0..4 {
                color_sum[i] += pattern_color[i] as f32;
            }
            num_patterns += 1;
        }
        let mut mean_color = [0; 4];
        for i in 0..4 {
            mean_color[i] = (color_sum[i] / num_patterns as f32).floor() as u8;
        }
        *color_lattice.get_mut_local(&p) = integer_from_rgba(Rgba(mean_color));
    }

    color_lattice
}

pub fn color_final_patterns(lattice: &Lattice<PatternId>, colors: &PatternColors) -> Lattice<u32> {
    let mut color_lattice = Lattice::fill(lattice.get_extent(), 0);
    for p in &color_lattice.get_extent() {
        let pattern = lattice.get_world(&p);
        let color = colors.get(*pattern);
        *color_lattice.get_mut_local(&p) = integer_from_rgba(Rgba(*color));
    }

    color_lattice
}
