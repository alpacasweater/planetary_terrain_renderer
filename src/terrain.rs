//! Types for configuring terrains.
//!

use crate::{
    math::{TerrainShape, TileCoordinate},
    terrain_data::{AttachmentConfig, AttachmentLabel},
};
use bevy::{ecs::entity::hash_map::EntityHashMap, platform::collections::HashMap, prelude::*};
use ron::{de::from_str, ser::to_string_pretty};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

pub const CURRENT_TERRAIN_FORMAT_VERSION: u32 = 2;
pub const CURRENT_GEODETIC_MAPPING_VERSION: u32 = 2;

const fn legacy_terrain_format_version() -> u32 {
    1
}

const fn legacy_geodetic_mapping_version() -> u32 {
    1
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileAvailability {
    #[default]
    Explicit,
    FullFace,
}

impl TileAvailability {
    pub fn contains(
        self,
        tile: TileCoordinate,
        shape: TerrainShape,
        lod_count: u32,
        explicit_tiles: impl FnOnce() -> bool,
    ) -> bool {
        if tile == TileCoordinate::INVALID
            || tile.face >= shape.face_count()
            || tile.lod >= lod_count
        {
            return false;
        }

        let tile_count = 1_i32 << tile.lod;
        if tile.xy.x < 0 || tile.xy.y < 0 || tile.xy.x >= tile_count || tile.xy.y >= tile_count {
            return false;
        }

        match self {
            TileAvailability::Explicit => explicit_tiles(),
            TileAvailability::FullFace => true,
        }
    }

    pub fn tile_count(
        self,
        shape: TerrainShape,
        lod_count: u32,
        explicit_tile_count: usize,
    ) -> usize {
        match self {
            TileAvailability::Explicit => explicit_tile_count,
            TileAvailability::FullFace => {
                let face_count = shape.face_count() as usize;
                (0..lod_count)
                    .map(|lod| face_count * (1_usize << (2 * lod)))
                    .sum()
            }
        }
    }
}

/// Resource that stores components that are associated to a terrain entity.
/// This is used to persist components in the render world.
#[derive(Deref, DerefMut, Resource)]
pub struct TerrainComponents<C>(EntityHashMap<C>);

impl<C> Default for TerrainComponents<C> {
    fn default() -> Self {
        Self(default())
    }
}

/// The configuration of a terrain.
///
/// Here you can define all fundamental parameters of the terrain.
#[derive(Serialize, Deserialize, Asset, TypePath, Debug, Clone)]
pub struct TerrainConfig {
    #[serde(default = "legacy_terrain_format_version")]
    pub format_version: u32,
    #[serde(default = "legacy_geodetic_mapping_version")]
    pub geodetic_mapping_version: u32,
    /// The path to the terrain folder inside the assets directory.
    pub path: String,
    pub shape: TerrainShape,
    /// The count of level of detail layers.
    pub lod_count: u32,
    pub min_height: f32,
    pub max_height: f32,
    /// The attachments of the terrain.
    pub attachments: HashMap<AttachmentLabel, AttachmentConfig>,
    #[serde(default)]
    pub tile_availability: TileAvailability,
    /// The tiles of the terrain.
    pub tiles: Vec<TileCoordinate>,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            format_version: CURRENT_TERRAIN_FORMAT_VERSION,
            geodetic_mapping_version: CURRENT_GEODETIC_MAPPING_VERSION,
            shape: TerrainShape::Plane { side_length: 1.0 },
            lod_count: 1,
            min_height: 0.0,
            max_height: 1.0,
            path: default(),
            tiles: default(),
            attachments: default(),
            tile_availability: TileAvailability::Explicit,
        }
    }
}

impl TerrainConfig {
    pub fn add_attachment(
        &mut self,
        label: AttachmentLabel,
        attachment: AttachmentConfig,
    ) -> &mut Self {
        self.attachments.insert(label, attachment);
        self
    }

    pub fn load_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let encoded = fs::read_to_string(path)?;
        Ok(from_str(&encoded)?)
    }

    pub fn save_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let encoded = to_string_pretty(self, default())?;
        Ok(fs::write(path, encoded)?)
    }

    pub fn is_tile_available(&self, tile: TileCoordinate) -> bool {
        self.tile_availability
            .contains(tile, self.shape, self.lod_count, || {
                self.tiles.contains(&tile)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CURRENT_GEODETIC_MAPPING_VERSION, CURRENT_TERRAIN_FORMAT_VERSION, TerrainConfig,
        TileAvailability,
    };
    use crate::math::{TerrainShape, TileCoordinate};
    use bevy::math::IVec2;
    use ron::de::from_str;

    #[test]
    fn terrain_config_default_uses_current_versions() {
        let config = TerrainConfig::default();
        assert_eq!(config.format_version, CURRENT_TERRAIN_FORMAT_VERSION);
        assert_eq!(
            config.geodetic_mapping_version,
            CURRENT_GEODETIC_MAPPING_VERSION
        );
    }

    #[test]
    fn legacy_terrain_config_defaults_to_mapping_version_one() {
        let encoded = r#"
(
    path: "assets/terrains/legacy",
    shape: Plane(side_length: 1.0),
    lod_count: 1,
    min_height: 0.0,
    max_height: 1.0,
    attachments: {},
    tiles: [],
)
"#;

        let config: TerrainConfig = from_str(encoded).expect("legacy terrain config should load");
        assert_eq!(config.format_version, 1);
        assert_eq!(config.geodetic_mapping_version, 1);
        assert_eq!(config.tile_availability, TileAvailability::Explicit);
    }

    #[test]
    fn procedural_full_face_tile_availability_accepts_in_bounds_tiles() {
        let config = TerrainConfig {
            shape: TerrainShape::WGS84,
            lod_count: 4,
            tile_availability: TileAvailability::FullFace,
            ..Default::default()
        };

        assert!(config.is_tile_available(TileCoordinate::new(5, 3, IVec2::new(7, 7))));
        assert!(!config.is_tile_available(TileCoordinate::new(6, 3, IVec2::new(7, 7))));
        assert!(!config.is_tile_available(TileCoordinate::new(5, 4, IVec2::new(0, 0))));
        assert!(!config.is_tile_available(TileCoordinate::new(5, 3, IVec2::new(8, 7))));
    }

    #[test]
    fn explicit_tile_availability_only_accepts_listed_tiles() {
        let listed_tile = TileCoordinate::new(0, 1, IVec2::new(1, 1));
        let config = TerrainConfig {
            shape: TerrainShape::WGS84,
            lod_count: 3,
            tile_availability: TileAvailability::Explicit,
            tiles: vec![listed_tile],
            ..Default::default()
        };

        assert!(config.is_tile_available(listed_tile));
        assert!(!config.is_tile_available(TileCoordinate::new(0, 1, IVec2::new(1, 0))));
    }
}
