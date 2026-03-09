use crate::{
    dataset::{PreprocessDataType, PreprocessNoData},
    gdal_extension::ProgressCallback,
};
use bevy_terrain::prelude::*;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;

const BAR_SIZE: u64 = 10000;
const ABOUT: &str = "Convert GDAL-readable rasters into bevy_terrain datasets.";
const LONG_ABOUT: &str = "\
Convert one or more source rasters into tiled terrain attachments under assets/terrains/*.

If you only want to render the bundled starter Earth, you do not need this tool.
Use the preprocess path when you want to build your own terrain assets from GeoTIFFs or
other GDAL-readable rasters.";
const AFTER_HELP: &str = "\
Starter paths:
  Render the bundled demo:
    cargo run --example minimal_globe

  Build the tutorial dataset from committed sample rasters:
    cargo run -p bevy_terrain_preprocess --example preprocess_tutorial_earth
    cargo run --example minimal_globe -- terrains/tutorial_earth

CLI examples:
  Height attachment:
    cargo run -p bevy_terrain_preprocess -- sample_data/gebco_earth_mini.tif assets/terrains/tutorial_earth --overwrite --lod-count 3 --attachment-label height --format r32f --ts 128 --bs 4 --m 4

  Albedo attachment:
    cargo run -p bevy_terrain_preprocess -- sample_data/true_marble_mini.tif assets/terrains/tutorial_earth --overwrite --lod-count 3 --attachment-label albedo --format rg8u --ts 128 --bs 4 --m 4

The preprocess crate requires GDAL, but it no longer requires libclang/bindgen for supported
GDAL versions.";

#[derive(Parser, Debug)]
#[command(
    name = "bevy_terrain_preprocess",
    author,
    version,
    about = ABOUT,
    long_about = LONG_ABOUT,
    after_help = AFTER_HELP
)]
pub struct Cli {
    #[arg(
        required = true,
        value_name = "SRC_PATH",
        help = "One or more source rasters or directories of rasters."
    )]
    pub src_path: Vec<PathBuf>,
    #[arg(required = true)]
    #[arg(
        value_name = "TERRAIN_PATH",
        help = "Output terrain directory, usually assets/terrains/<name>."
    )]
    // could be optional and use current directory, but this would be risky in combination with overwrite
    pub terrain_path: PathBuf,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional temp directory for intermediate warp output."
    )]
    pub temp_path: Option<PathBuf>,

    #[arg(
        short,
        long,
        default_value_t = false,
        help = "Overwrite existing attachment output."
    )]
    pub overwrite: bool,
    #[arg(
        long,
        default_value_t = false,
        help = "Keep temporary files instead of cleaning them up."
    )]
    pub keep_temp: bool,
    #[arg(
        long,
        default_value = "source",
        help = "No-data handling. Use `source`, `alpha`, or a numeric value."
    )]
    pub no_data: PreprocessNoData,
    #[arg(
        long,
        default_value = "source",
        help = "Output GDAL data type. Use `source` or a concrete GDAL type such as `Float32`."
    )]
    pub data_type: PreprocessDataType,
    #[arg(
        long,
        default_value_t = 16.0,
        help = "Fill radius in pixels for gap filling."
    )]
    pub fill_radius: f32,
    #[arg(
        long,
        default_value_t = false,
        help = "Generate a tile mask attachment."
    )]
    pub create_mask: bool,

    #[arg(
        long,
        help = "Optional LOD count override. Starter values around 3 are fast to iterate on."
    )]
    pub lod_count: Option<u32>,

    #[arg(
        long,
        default_value = "height",
        help = "Attachment label. Use `height` for elevation or a custom label like `albedo`."
    )]
    pub attachment_label: AttachmentLabel,
    #[arg(
        short,
        long = "ts",
        default_value_t = 512,
        help = "Tile texture size including borders."
    )]
    pub texture_size: u32,
    #[arg(
        short,
        long = "bs",
        default_value_t = 1,
        help = "Border overlap in texels used for seamless sampling."
    )]
    pub border_size: u32,
    #[arg(
        short,
        long = "m",
        default_value_t = 1,
        help = "Number of mip levels to write per tile."
    )]
    pub mip_level_count: u32,
    #[arg(
        long,
        default_value = "r16u",
        help = "Attachment texture format, for example `r32f`, `r16u`, or `rg8u`."
    )]
    pub format: AttachmentFormat,
}

pub(crate) struct PreprocessBar<'a> {
    name: String,
    bar: ProgressBar,
    callback: Box<ProgressCallback<'a>>,
}

impl PreprocessBar<'_> {
    pub(crate) fn new(name: String) -> Self {
        let bar = ProgressBar::new(BAR_SIZE).with_style(
            ProgressStyle::with_template(
                &(name.clone() + " dataset: {wide_bar} {percent} % [{elapsed}/{duration}])"),
            )
            .unwrap(),
        );

        let callback = Box::new({
            let progress_bar = bar.clone();
            move |completion| {
                progress_bar.set_position((completion * BAR_SIZE as f64) as u64);
                true
            }
        });

        Self {
            name,
            bar,
            callback,
        }
    }

    pub(crate) fn callback(&self) -> &ProgressCallback<'_> {
        self.callback.as_ref()
    }

    pub(crate) fn finish(&self) {
        self.bar.finish_and_clear();
        println!("{} took: {:?}", self.name, self.bar.elapsed());
    }
}
