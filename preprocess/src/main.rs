use bevy_terrain_preprocess::prelude::*;
use clap::Parser;
use std::env::set_var;

fn main() {
    unsafe {
        if true {
            set_var("RAYON_NUM_THREADS", "0");
            // The preprocess path uses a custom GDAL transformer. Until that transformer
            // supports real per-thread cloning, keep GDAL warp single-threaded to avoid
            // unsafe concurrent use of libproj state.
            set_var("GDAL_NUM_THREADS", "1");
        } else {
            set_var("RAYON_NUM_THREADS", "1");
            set_var("GDAL_NUM_THREADS", "1");
        }
    }

    let args = Cli::parse();
    let (src_dataset, mut context) = match PreprocessContext::from_cli(args) {
        Ok(values) => values,
        Err(error) => {
            eprintln!("preprocess setup failed: {error:?}");
            std::process::exit(1);
        }
    };

    if let Err(error) = preprocess(src_dataset, &mut context) {
        eprintln!("preprocess failed: {error:?}");
        std::process::exit(1);
    }
}
