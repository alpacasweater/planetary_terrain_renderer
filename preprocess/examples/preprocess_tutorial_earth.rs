//! Build a tiny tutorial Earth dataset from committed sample rasters.
//!
//! This example is the intended first preprocess run for new users because it
//! requires no external downloads once GDAL is installed.

use bevy_terrain::prelude::*;
use bevy_terrain_preprocess::prelude::*;
use gdal::raster::GdalDataType;

const TERRAIN_PATH: &str = "assets/terrains/tutorial_earth";
const HEIGHT_SOURCE: &str = "sample_data/gebco_earth_mini.tif";
const ALBEDO_SOURCE: &str = "sample_data/true_marble_mini.tif";

fn main() {
    preprocess_attachment(Cli {
        src_path: vec![HEIGHT_SOURCE.into()],
        terrain_path: TERRAIN_PATH.into(),
        temp_path: None,
        keep_temp: false,
        overwrite: true,
        no_data: PreprocessNoData::Source,
        data_type: PreprocessDataType::DataType(GdalDataType::Float32),
        fill_radius: 0.0,
        create_mask: false,
        lod_count: Some(3),
        attachment_label: AttachmentLabel::Height,
        texture_size: 128,
        border_size: 4,
        mip_level_count: 4,
        format: AttachmentFormat::R32F,
    });

    preprocess_attachment(Cli {
        src_path: vec![ALBEDO_SOURCE.into()],
        terrain_path: TERRAIN_PATH.into(),
        temp_path: None,
        keep_temp: false,
        overwrite: true,
        no_data: PreprocessNoData::Source,
        data_type: PreprocessDataType::Source,
        fill_radius: 0.0,
        create_mask: false,
        lod_count: Some(3),
        attachment_label: AttachmentLabel::Custom("albedo".to_string()),
        texture_size: 128,
        border_size: 4,
        mip_level_count: 4,
        format: AttachmentFormat::Rgb8U,
    });
}

fn preprocess_attachment(args: Cli) {
    let (src_dataset, mut context) =
        PreprocessContext::from_cli(args).expect("preprocess arguments should be valid");
    preprocess(src_dataset, &mut context).expect("preprocess should succeed");
}
