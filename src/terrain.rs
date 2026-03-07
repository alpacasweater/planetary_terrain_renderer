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
}

#[cfg(test)]
mod tests {
    use super::{
        CURRENT_GEODETIC_MAPPING_VERSION, CURRENT_TERRAIN_FORMAT_VERSION, TerrainConfig,
    };
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
    }
}
