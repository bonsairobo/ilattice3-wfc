use ilattice3_wfc::*;

use dot_vox::DotVoxData;
use flexi_logger::{default_format, Logger};
use ilattice3 as lat;
use ilattice3::{Lattice, PeriodicYLevelsIndexer, VoxColor};
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

    /// If the input lattice contains tiles (repeated patterns larger than 1 voxel), set this size
    /// to capture that structure. This is also much more efficient.
    #[structopt(short, long)]
    tile_size: Vec<i32>,

    /// Size of the patterns (in tiles) to extract from the input data. E.g. if tile size is 2x2x2
    /// and pattern size is 2x1x1 then the full size of a pattern in voxels is 4x2x2.
    #[structopt(short, long)]
    pattern_size: Vec<i32>,

    /// Size of the generated output in tiles.
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
fn main(args: Args) -> Result<(), CliError> {
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

    let ProcessedInput {
        input_lattice,
        tile_size,
        pattern_shape,
        seed,
        output_size,
    } = process_args(&args)?;

    match input_lattice {
        InputLattice::Vox(lattice, color_palette) => generate_vox(
            args,
            seed,
            tile_size,
            pattern_shape,
            lattice,
            output_size,
            color_palette,
            running,
        )?,
        InputLattice::Image(lattice) => generate_image(
            args,
            seed,
            tile_size,
            pattern_shape,
            lattice,
            output_size,
            running,
        )?,
    }

    Ok(())
}

struct ProcessedInput<I> {
    input_lattice: InputLattice<I>,
    tile_size: lat::Point,
    pattern_shape: PatternShape,
    seed: [u8; NUM_SEED_BYTES],
    output_size: lat::Point,
}

enum InputLattice<I> {
    // Vox lattice stores indices into a color palette.
    Vox(Lattice<VoxColor, I>, VoxColorPalette),
    // Images just store the colors directly.
    Image(Lattice<u32, I>),
}

struct VoxColorPalette {
    colors: Vec<u32>,
}

fn process_args(args: &Args) -> Result<ProcessedInput<PeriodicYLevelsIndexer>, CliError> {
    let indexer = PeriodicYLevelsIndexer {};

    if !tile_size_is_valid(&args.tile_size) {
        panic!("Voxel size must specify 3 positive dimensions");
    }
    if !tile_size_is_valid(&args.pattern_size) {
        panic!("Pattern size must specify 3 positive dimensions");
    }
    if !tile_size_is_valid(&args.output_size) {
        panic!("Output size must specify 3 positive dimensions");
    }
    let tile_size = lat::Point::from(get_three_elements(&args.tile_size));
    let pattern_size = lat::Point::from(get_three_elements(&args.pattern_size));
    let output_size = lat::Point::from(get_three_elements(&args.output_size));

    if args.gif.is_some() && output_size.z > 2 {
        panic!("GIF output not supported for 3D output");
    }

    let mut seed = [0; NUM_SEED_BYTES];
    let seed_bytes = args.seed.as_bytes();
    let copy_bytes = seed_bytes.len().min(NUM_SEED_BYTES);
    seed[..copy_bytes].clone_from_slice(&seed_bytes[..copy_bytes]);

    let extension = args
        .input_path
        .extension()
        .expect("Input file has no extention");
    let (input_lattice, offsets) = if extension == "vox" {
        assert!(
            args.palette.is_none(),
            "Palette image only supported for 2D images"
        );
        let input_vox =
            dot_vox::load(args.input_path.to_str().unwrap()).expect("Failed to load VOX file");
        let model_index = 0;

        (
            InputLattice::Vox(
                Lattice::from_vox_with_indexer(indexer, &input_vox, model_index),
                VoxColorPalette {
                    colors: input_vox.palette,
                },
            ),
            face_3d_offsets(),
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
        let input_img = image::open(args.input_path.as_os_str())?;

        (
            InputLattice::Image(lattice_from_image(indexer, &input_img.to_rgba())),
            edge_2d_offsets(),
        )
    };

    Ok(ProcessedInput {
        input_lattice,
        tile_size,
        pattern_shape: PatternShape {
            size: pattern_size,
            offset_group: OffsetGroup::new(&offsets),
        },
        seed,
        output_size,
    })
}

fn tile_size_is_valid(size: &Vec<i32>) -> bool {
    for c in size.iter() {
        if *c <= 0 {
            return false;
        }
    }

    size.len() == 3
}

fn get_three_elements(v: &[i32]) -> [i32; 3] {
    assert_eq!(v.len(), 3);
    v.iter().for_each(|e| assert!(*e >= 0));

    let mut elems = [0; 3];
    elems[..].clone_from_slice(v);

    elems
}

fn generate_image(
    args: Args,
    seed: [u8; 16],
    tile_size: lat::Point,
    pattern_shape: PatternShape,
    input_lattice: Lattice<u32, PeriodicYLevelsIndexer>,
    output_size: lat::Point,
    running: Arc<AtomicBool>,
) -> Result<(), CliError> {
    println!(
        "Input size in voxels = {}",
        input_lattice.get_extent().get_local_supremum()
    );

    let (pattern_group, representatives) =
        process_patterns_in_lattice(&input_lattice, &tile_size, &pattern_shape);
    println!(
        "Found {} patterns in input lattice",
        pattern_group.num_patterns()
    );

    if let Some(palette_path) = args.palette {
        // Save the palette image for debugging.
        let palette_lattice = make_palette_lattice(&input_lattice, &representatives);
        let palette_img = image_from_lattice(&palette_lattice);
        palette_img.save(palette_path)?;
    }

    let pattern_tiles = find_pattern_tiles_image(&input_lattice, &representatives, &tile_size);

    let skip_frames = args.skip_frames;
    let mut gif_maker = args
        .gif
        .map(|gif_path| GifMaker::new(gif_path, pattern_tiles.clone(), tile_size, skip_frames));

    if let Some(result) = generate(seed, &pattern_group, output_size, &mut gif_maker, running) {
        assert!(pattern_group.constraints.assignment_is_valid(&result));
        let colors = color_final_patterns_rgba(&result, &pattern_tiles, &tile_size);
        let final_img = image_from_lattice(&colors);
        println!("Writing {:?}", args.output_path);
        final_img.save(args.output_path)?;

        if let Some(maker) = gif_maker {
            maker.save()?;
        }
    }

    Ok(())
}

fn generate_vox(
    args: Args,
    seed: [u8; 16],
    tile_size: lat::Point,
    pattern_shape: PatternShape,
    input_lattice: Lattice<VoxColor, PeriodicYLevelsIndexer>,
    output_size: lat::Point,
    color_palette: VoxColorPalette,
    running: Arc<AtomicBool>,
) -> Result<(), std::io::Error> {
    println!(
        "Input size = {}",
        input_lattice.get_extent().get_local_supremum()
    );

    let (pattern_group, representatives) =
        process_patterns_in_lattice(&input_lattice, &tile_size, &pattern_shape);
    println!(
        "Found {} patterns in input lattice",
        pattern_group.num_patterns()
    );

    let pattern_tiles = find_pattern_tiles_vox(&input_lattice, &representatives, &tile_size);

    if let Some(result) =
        generate::<NilFrameConsumer>(seed, &pattern_group, output_size, &mut None, running)
    {
        let colors = color_final_patterns_vox(&result, &pattern_tiles, &tile_size);
        let mut vox_data: DotVoxData = colors.into();
        vox_data.palette = color_palette.colors;
        println!("Writing {:?}", args.output_path);
        let mut out_file = File::create(args.output_path)?;
        vox_data.write_vox(&mut out_file)?;
    }

    Ok(())
}

fn generate<F>(
    seed: [u8; 16],
    pattern_group: &PatternGroup,
    output_size: lat::Point,
    frame_consumer: &mut Option<F>,
    running: Arc<AtomicBool>,
) -> Option<Lattice<PatternId>>
where
    F: FrameConsumer,
{
    println!("Trying to generate with seed {:?}", seed);

    let volume = lat::Extent::from_min_and_local_supremum([0, 0, 0].into(), output_size).volume();
    let progress_bar = ProgressBar::new(volume as u64);

    let mut generator = Generator::new(seed, output_size, pattern_group);
    let mut success = true;
    println!("Generating...");
    loop {
        let state = generator.update(pattern_group);
        progress_bar.set_position(generator.num_collapsed() as u64);
        match state {
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

        if let Some(consumer) = frame_consumer {
            consumer.use_frame(generator.get_wave_lattice());
        }
    }

    progress_bar.finish_at_current_pos();

    if success {
        Some(generator.result())
    } else {
        println!("Failed to generate");

        None
    }
}
