use ilattice3_wfc::*;

use flexi_logger::{Logger, default_format};
use ilattice3 as lat;
use ilattice3::{Lattice, PeriodicYLevelsIndexer};
use image::{self, gif, Delay, Frame};
use indicatif::ProgressBar;
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(structopt::StructOpt)]
struct Args {
    /// Path to the input file, either an image or a VOX file.
    #[structopt(parse(from_os_str))]
    input_path: PathBuf,

    /// Path to the output file.
    #[structopt(parse(from_os_str))]
    output_path: PathBuf,

    /// Size of the patterns to extract from the input data.
    #[structopt(short, long)]
    pattern_size: Vec<i32>,

    /// Size of the generated output.
    #[structopt(short, long)]
    output_size: Vec<i32>,

    /// A 32-byte string serving as the seed for the random number generator. Results are
    /// reproducible from a given seed.
    #[structopt(short, long, default_value = "1")]
    seed: String,

    /// Produce an animated GIF showing each update of the generator algorithm.
    #[structopt(short, long, parse(from_os_str))]
    gif: Option<PathBuf>,

    /// Take one GIF frame for every N updates of the generator.
    #[structopt(long, default_value = "1")]
    skip_frames: usize,

    /// Path where the pattern palette image should be saved. Only supported for 2D images.
    #[structopt(long, parse(from_os_str))]
    palette: Option<PathBuf>,

    /// A log config string, e.g. "info" or "debug, module = trace".
    #[structopt(short, long)]
    log: Option<String>,
}

#[paw::main]
fn main(args: Args) -> Result<(), std::io::Error> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || r.store(false, Ordering::SeqCst))
        .expect("Failed to register SIGINT handler");

    if let Some(log_config) = &args.log {
        Logger::with_str(log_config.as_str())
            .log_to_file()
            .format(default_format)
            .start()
            .unwrap_or_else(|e| panic!("Logger initialization failed with {}", e));
    }

    let input = process_args(&args);

    generate(args, input, running)?;

    Ok(())
}

struct ProcessedInput<I> {
    input_lattice: Lattice<u32, I>,
    pattern_shape: PatternShape,
    seed: [u8; NUM_SEED_BYTES],
    output_size: lat::Point,
    num_dimensions: usize,
}

fn process_args(args: &Args) -> ProcessedInput<PeriodicYLevelsIndexer> {
    let indexer = PeriodicYLevelsIndexer {};

    if args.pattern_size.len() != 3 {
        panic!("Pattern size must specify 3 dimensions");
    }
    if args.output_size.len() != 3 {
        panic!("Output size must specify 3 dimensions");
    }
    let pattern_size = lat::Point::from(get_three_elements(&args.pattern_size));
    let output_size = lat::Point::from(get_three_elements(&args.output_size));

    if args.gif.is_some() && output_size.z > 2 {
        panic!("GIF output not supported for 3D output");
    }

    let mut seed = [0; NUM_SEED_BYTES];
    let seed_bytes = args.seed.as_bytes();
    seed[..seed_bytes.len().min(NUM_SEED_BYTES)].clone_from_slice(seed_bytes);

    let extension = args
        .input_path
        .extension()
        .expect("Input file has no extention");
    let (input_lattice, offset_group, num_dimensions) = if extension == "vox" {
        assert!(
            args.palette.is_none(),
            "Palette image only supported for 2D images"
        );
        let input_vox =
            dot_vox::load(args.input_path.to_str().unwrap()).expect("Failed to load VOX file");

        (
            Lattice::from_vox_with_indexer(indexer, &input_vox, 0),
            OffsetGroup::new(&face_3d_offsets()),
            3,
        )
    } else {
        assert_eq!(
            pattern_size.z, 1,
            "3D images not supported, use --pattern-size x y 1"
        );
        assert_eq!(
            output_size.z, 1,
            "3D images not supported, use --output-size x y 1"
        );
        let input_img = image::open(args.input_path.as_os_str()).unwrap();

        (
            lattice_from_image(indexer, &input_img.to_rgba()),
            OffsetGroup::new(&edge_2d_offsets()),
            2,
        )
    };

    ProcessedInput {
        input_lattice,
        pattern_shape: PatternShape {
            size: pattern_size,
            offset_group,
        },
        seed,
        output_size,
        num_dimensions,
    }
}

fn get_three_elements(v: &[i32]) -> [i32; 3] {
    assert_eq!(v.len(), 3);
    v.iter().for_each(|e| assert!(*e >= 0));

    let mut elems = [0; 3];
    elems[..].clone_from_slice(v);

    elems
}

fn generate(
    args: Args,
    input: ProcessedInput<PeriodicYLevelsIndexer>,
    running: Arc<AtomicBool>,
) -> Result<(), std::io::Error> {
    let ProcessedInput {
        input_lattice,
        pattern_shape,
        seed,
        output_size,
        num_dimensions,
    } = input;

    println!("Trying to generate with seed {:?}", seed);

    let (pattern_group, representatives) =
        process_patterns_in_lattice(&input_lattice, &pattern_shape);
    println!("Found {} patterns in input lattice", pattern_group.num_patterns());

    if let Some(palette_path) = args.palette {
        // Save the palette image for debugging.
        let palette_lattice = make_palette_lattice(&input_lattice, &representatives);
        let palette_img = image_from_lattice(&palette_lattice);
        palette_img.save(palette_path).unwrap();
    }

    let pattern_colors = find_pattern_colors(&input_lattice, &representatives);

    let volume = lat::Extent::from_min_and_local_supremum([0, 0, 0].into(), output_size).volume();
    let progress_bar = ProgressBar::new(volume as u64);

    let mut generator = Generator::new(seed, output_size, &pattern_group);
    let mut frames = Vec::new();
    let mut success = true;
    let mut num_updates = 0;
    println!("Starting generator");
    loop {
        match generator.update(&pattern_group) {
            UpdateResult::Success => break,
            UpdateResult::Failure => {
                success = false;
                break;
            }
            UpdateResult::Continue => (),
        }
        // Can be interrupted by other threads.
        if !running.load(Ordering::SeqCst) {
            success = false;
            break;
        }

        progress_bar.set_position(generator.num_collapsed() as u64);

        if args.gif.is_some() && num_updates % args.skip_frames == 0 {
            let superposition = color_superposition(generator.get_wave_lattice(), &pattern_colors);
            let superposition_img = image_from_lattice(&superposition);
            frames.push(Frame::from_parts(
                superposition_img,
                0,
                0,
                Delay::from_numer_denom_ms(1, 1),
            ));
        }

        num_updates += 1;
    }

    progress_bar.finish();

    if success {
        // TODO: support saving 3D lattice for viewing (RON format?)
        if num_dimensions == 3 {
            return Ok(());
        }

        let result = generator.result();
        let colors = color_final_patterns(&result, &pattern_colors);
        let final_img = image_from_lattice(&colors);
        println!("Writing {:?}", args.output_path);
        final_img
            .save(args.output_path)
            .expect("Failed to save output image");
    } else {
        println!("Failed to generate");
    }

    if let Some(gif_path) = args.gif {
        println!("Writing {:?}", gif_path);
        let file_out = File::create(&gif_path).unwrap();
        gif::Encoder::new(file_out)
            .encode_frames(frames.into_iter())
            .unwrap();
    }

    Ok(())
}
