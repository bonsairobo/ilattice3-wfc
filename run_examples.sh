#!/bin/bash
set -e

cargo build --release

RUST_BACKTRACE=1 target/release/cli test_data/flowers.png out_flowers.png -o 128 128 1 -s flowerdaddy -p 2 2 1 -t 1 1 1
diff out_flowers.png test_data/output/flowers_flowerdaddy.png
rm out_flowers.png

RUST_BACKTRACE=1 target/release/cli test_data/flowers.png out_flowers.png -o 128 128 1 -s flowermomma -p 2 2 1 -t 2 2 1
diff out_flowers.png test_data/output/flowers_flowermomma.png
rm out_flowers.png

RUST_BACKTRACE=1 target/release/cli test_data/monu10.vox out_monu10.vox -o 10 10 20 -s monudaddy -p 2 2 2 -t 8 8 8
diff out_monu10.vox test_data/output/monu10_monudaddy.vox
rm out_monu10.vox

RUST_BACKTRACE=1 target/release/cli test_data/monu10.vox out_monu10.vox -o 10 10 20 -s monumomma -p 2 2 2 -t 8 8 8
diff out_monu10.vox test_data/output/monu10_monubaby.vox
rm out_monu10.vox
