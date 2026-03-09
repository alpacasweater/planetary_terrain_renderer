use crate::{math::TileCoordinate, terrain_data::AttachmentLabel};
use std::path::{Path, PathBuf};

pub fn attachment_relative_root(terrain_path: &str, attachment_label: &AttachmentLabel) -> PathBuf {
    Path::new(terrain_path).join(String::from(attachment_label))
}

pub fn starter_tile_asset_path(
    terrain_path: &str,
    attachment_label: &AttachmentLabel,
    coordinate: TileCoordinate,
) -> PathBuf {
    coordinate.path(&attachment_relative_root(terrain_path, attachment_label))
}

pub fn cache_tile_asset_path(
    cache_root: &Path,
    terrain_path: &str,
    attachment_label: &AttachmentLabel,
    coordinate: TileCoordinate,
) -> PathBuf {
    coordinate.path(&cache_root.join(attachment_relative_root(terrain_path, attachment_label)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::IVec2;

    #[test]
    fn starter_paths_match_existing_tile_layout() {
        let path = starter_tile_asset_path(
            "terrains/earth",
            &AttachmentLabel::Custom("albedo".to_string()),
            TileCoordinate::new(3, 4, IVec2::new(5, 7)),
        );
        assert_eq!(
            path,
            PathBuf::from("terrains/earth/albedo/4/0_0/3_4_5_7.tif")
        );
    }

    #[test]
    fn cache_paths_prefix_the_cache_root() {
        let path = cache_tile_asset_path(
            Path::new("streaming_cache"),
            "terrains/earth",
            &AttachmentLabel::Height,
            TileCoordinate::new(1, 2, IVec2::new(3, 0)),
        );
        assert_eq!(
            path,
            PathBuf::from("streaming_cache/terrains/earth/height/2/0_0/1_2_3_0.tif")
        );
    }
}
